use anyhow::{Context, Result};
use reqwest::Client;
use std::time::Duration;

/// The outcome of fetching a signed URL with no credentials — exactly what
/// whoever receives the link will do.
pub struct Probe {
    pub status: u16,
    /// Only populated on failure. GCS's XML error body names the exact
    /// missing permission, which is what makes a 403 here diagnosable
    /// instead of just "it didn't work".
    pub body: Option<String>,
}

impl Probe {
    pub fn is_ok(&self) -> bool {
        (200..300).contains(&self.status)
    }
}

/// Fetches `url` with no credentials, mirroring the recipient.
///
/// This uses a ranged GET rather than a bare HEAD. A HEAD response never
/// carries a body — success or failure — so a HEAD-only probe would tell us
/// THAT the link failed but never WHY. `Range: bytes=0-0` keeps the request
/// as cheap as a HEAD on success (a 206 with one byte) while still getting
/// GCS's explanatory XML body back on failure.
pub async fn probe(url: &str) -> Result<Probe> {
    let client = Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .context("failed to build an HTTP client to verify the signed URL")?;

    let resp = client
        .get(url)
        .header(reqwest::header::RANGE, "bytes=0-0")
        .send()
        .await
        .map_err(|e| e.without_url())
        .context("failed to reach the signed URL to verify it works")?;

    let status = resp.status().as_u16();
    let body = if (200..300).contains(&status) {
        None
    } else {
        resp.text()
            .await
            .ok()
            .map(|t| t.chars().take(2000).collect())
    };
    Ok(Probe { status, body })
}

/// Pure: turns a failed probe into actionable advice. Split out from
/// `probe` so this is unit-testable without a network call.
pub fn explain_failure(status: u16, body: Option<&str>, bucket: &str) -> String {
    let body_block = match body {
        Some(b) if !b.trim().is_empty() => format!("\n\nGCS said:\n\n{}", indent(b.trim())),
        _ => String::new(),
    };

    if status == 403 {
        format!(
            "The upload succeeded and a signed URL was produced, but fetching that URL with no \
             credentials returned 403 Forbidden — that is exactly what whoever you send this \
             link to would see. Nothing was printed, because a link that does not work is worse \
             than no link.\n\
             \n\
             A V4 signed URL is authorized as the identity that signed it, not just by having a \
             valid signature. This signer can create objects in gs://{bucket} but cannot read \
             them back: a GET link needs storage.objects.get, which roles/storage.objectCreator \
             does not grant.\n\
             \n\
             Grant roles/storage.objectViewer on the bucket, IN ADDITION to objectCreator:\n\
             \n\
             \x20   gcloud storage buckets add-iam-policy-binding gs://{bucket} \\\n\
             \x20     --member='serviceAccount:SA@PROJECT.iam.gserviceaccount.com' \\\n\
             \x20     --role='roles/storage.objectViewer'\n\
             \n\
             ⚠️ objectViewer also grants storage.objects.list, so any identity holding it can \
             enumerate and read every object in the bucket, not just this run's upload. If this \
             service account is shared by other agents or people, give gcsdrop its own bucket \
             rather than sharing one with unrelated content.{body_block}"
        )
    } else {
        format!(
            "The upload succeeded and a signed URL was produced, but fetching that URL with no \
             credentials returned HTTP {status} instead of success — that is exactly what \
             whoever you send this link to would see. Nothing was printed, because a link that \
             does not work is worse than no link.{body_block}"
        )
    }
}

fn indent(s: &str) -> String {
    s.lines()
        .map(|l| format!("    {l}"))
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn two_hundred_is_ok() {
        assert!(Probe {
            status: 200,
            body: None
        }
        .is_ok());
    }

    #[test]
    fn partial_content_from_the_ranged_get_is_ok() {
        // The probe deliberately sends Range: bytes=0-0, so a healthy
        // signed URL answers 206, not 200 — that must still count as
        // success, not get treated as a failure.
        assert!(Probe {
            status: 206,
            body: None
        }
        .is_ok());
    }

    #[test]
    fn four_oh_three_is_not_ok() {
        assert!(!Probe {
            status: 403,
            body: None
        }
        .is_ok());
    }

    #[test]
    fn explain_403_names_object_viewer_and_the_bucket() {
        let msg = explain_failure(403, None, "my-bucket");
        assert!(msg.contains("roles/storage.objectViewer"), "{msg}");
        assert!(msg.contains("gs://my-bucket"), "{msg}");
        assert!(msg.contains("storage.objects.get"), "{msg}");
    }

    #[test]
    fn explain_403_warns_about_list_exposure() {
        // objectViewer includes storage.objects.list: anyone holding it can
        // enumerate the whole bucket, not just their own upload. That
        // consequence must not be buried or omitted.
        let msg = explain_failure(403, None, "my-bucket");
        assert!(msg.contains("storage.objects.list"), "{msg}");
        assert!(msg.contains("enumerate"), "{msg}");
    }

    #[test]
    fn explain_403_says_create_works_but_read_does_not() {
        let msg = explain_failure(403, None, "my-bucket");
        assert!(msg.contains("can create objects"), "{msg}");
        assert!(msg.contains("cannot read them back"), "{msg}");
    }

    #[test]
    fn explain_403_includes_the_real_gcs_body_when_present() {
        let body = "<Error><Code>AccessDenied</Code>\
                     <Message>Access denied.</Message>\
                     <Details>does not have storage.objects.get access</Details></Error>";
        let msg = explain_failure(403, Some(body), "my-bucket");
        assert!(msg.contains("storage.objects.get access"), "{msg}");
    }

    #[test]
    fn explain_403_tolerates_missing_body() {
        let msg = explain_failure(403, None, "my-bucket");
        assert!(!msg.contains("GCS said"), "{msg}");
    }

    #[test]
    fn explain_403_tolerates_empty_body() {
        let msg = explain_failure(403, Some("   "), "my-bucket");
        assert!(!msg.contains("GCS said"), "{msg}");
    }

    #[test]
    fn explain_non_403_names_the_status_code() {
        let msg = explain_failure(404, None, "my-bucket");
        assert!(msg.contains("404"), "{msg}");
        assert!(!msg.contains("objectViewer"), "{msg}");
    }

    #[tokio::test]
    async fn probe_transport_failure_does_not_leak_the_signature_in_the_error() {
        // Port 1 on loopback: nothing listens there, so this fails at the
        // transport layer (connection refused) with no real network
        // egress — hermetic and fast. Regression test for the bug where
        // reqwest::Error's Display embeds the full request URL, which
        // would leak the signature query string into stderr on a probe
        // failure (timeout, DNS, connection reset, TLS — all go through
        // this same Err path).
        let url = "http://127.0.0.1:1/object?X-Goog-Signature=super-secret-signature";
        let result = probe(url).await;
        let err = match result {
            Ok(_) => panic!("port 1 on loopback must not accept connections"),
            Err(e) => e,
        };
        let msg = format!("{err:#}");
        assert!(
            !msg.contains("super-secret-signature"),
            "error must not leak the signed URL's query string: {msg}"
        );
    }

    #[test]
    fn explain_never_pretends_the_link_works() {
        for status in [401, 403, 404, 500, 503] {
            let msg = explain_failure(status, None, "my-bucket");
            assert!(msg.contains("worse than no link"), "status {status}: {msg}");
        }
    }
}
