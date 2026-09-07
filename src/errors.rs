use crate::adc::{AdcKind, AdcSource};

/// Advice for credentials that cannot produce a signed URL.
pub fn cannot_sign(kind: &AdcKind) -> String {
    match kind {
        AdcKind::AuthorizedUser => "\
Your Application Default Credentials are a user account, which cannot sign URLs.

This is a Google limitation, not a gcsdrop one. gcloud refuses the same way:

    ERROR: (gcloud.storage.sign-url) This command requires a service account
    to sign a URL. Please authenticate with a service account, or provide the
    global '--impersonate-service-account' flag.

Fix it by impersonating a service account:

    gcloud auth application-default login \\
      --impersonate-service-account=SA@PROJECT.iam.gserviceaccount.com

⚠️ That login overwrites this machine's Application Default Credentials for
every tool that reads them, not just gcsdrop. Back up your current ones
first:

    cp ~/.config/gcloud/application_default_credentials.json ~/adc-backup.json 2>/dev/null \\
      || echo \"no existing ADC to back up — nothing to lose\"

You need roles/iam.serviceAccountTokenCreator on that service account, and
the service account needs it on itself."
            .to_string(),

        AdcKind::ExternalAccount => "\
Your Application Default Credentials come from Workload Identity Federation
(an `external_account` credential), which cannot sign URLs — the auth
library rejects that credential type for signing outright, even when the
credential's own `service_account_impersonation_url` is set. There is
usually no browser in a Workload Identity Federation environment either, so
an interactive `gcloud auth application-default login` is not a fix here.

What does work with this crate: wrap your external_account credential as
the `source_credentials` of an `impersonated_service_account` ADC config,
with `service_account_impersonation_url` set on the OUTER object (not
inside the external_account block) to target the service account you want
to sign as. How to obtain the external_account block itself depends on your
platform (GKE Workload Identity, AWS, Azure, ...) — see Google's Workload
Identity Federation docs:

    https://cloud.google.com/iam/docs/workload-identity-federation

You need roles/iam.serviceAccountTokenCreator on that service account, and
the service account needs it on itself."
            .to_string(),

        AdcKind::MissingCredentialsFile(path) => format!(
            "GOOGLE_APPLICATION_CREDENTIALS points at {}, which cannot be read.\n\
             \n\
             Fix it by either correcting the path or unsetting the variable:\n\
             \n\
             \x20   unset GOOGLE_APPLICATION_CREDENTIALS\n\
             \n\
             Then gcsdrop will fall back to the normal Application Default Credentials discovery.\n\
             \n\
             Or point it at a credentials file that does exist. `gcloud auth\n\
             application-default login` writes to:\n\
             \n\
             \x20   ~/.config/gcloud/application_default_credentials.json\n\
             \n\
             so that is the path to point at — or just unset the variable and let\n\
             gcsdrop find it there on its own.",
            path.display()
        ),

        other => format!(
            "Your Application Default Credentials ({other:?}) cannot sign URLs.\n\
             \n\
             Switch to credentials that can sign:\n\
             \n\
             \x20   gcloud auth application-default login \\\n\
             \x20     --impersonate-service-account=SA@PROJECT.iam.gserviceaccount.com"
        ),
    }
}

/// Recognises common permission failures and returns a fix.
/// Returns None when we have nothing useful to add.
///
/// `bucket` names the real bucket in the advice; the identity in the advice
/// stays a placeholder because resolving it needs a network call this
/// function does not make. `adc_source` is where the ADC file came from
/// (see [`crate::adc::adc_source`]) — needed for the credential-expiry
/// branch below, which gives different advice depending on it.
pub fn explain(
    err: &anyhow::Error,
    bucket: &str,
    adc_source: Option<&AdcSource>,
) -> Option<String> {
    let text = format!("{err:#}").to_lowercase();

    // Google requires periodic re-authentication ("reauth"). When it kicks
    // in, the refresh token backing the ADC stops working with
    // `invalid_rapt` specifically (reauth is required) or the broader
    // `invalid_grant` (this refresh token no longer works at all, for any
    // reason — reauth is one, but not the only one). Either way the fix is
    // the same: log in again. This is the single most common failure a
    // laptop user hits, since any credential built from a user account
    // ages out eventually; only metadata-server credentials on GCE/GKE are
    // immune.
    if text.contains("invalid_rapt") || text.contains("invalid_grant") {
        return Some(match adc_source {
            // GOOGLE_APPLICATION_CREDENTIALS set: by this project's own
            // convention (see README's "Local development" section) that
            // is the scoped impersonation file, not the plain gcloud ADC
            // file — and that file embeds its OWN copy of the user
            // credentials, so logging in again does not touch it.
            Some(AdcSource::FromEnv(path)) => {
                let path = path.display();
                format!(
                    "\
Your credentials have expired, and Google is asking you to log in again.

This is Google's periodic re-authentication ('reauth') policy — the error above
shows \"invalid_rapt\" and/or \"invalid_grant\", which mean your organization
requires it, not that gcsdrop or your setup is broken.

Logging in again is NOT enough by itself here: GOOGLE_APPLICATION_CREDENTIALS
is set to

    {path}

which is an impersonation file — it embeds a COPY of your user credentials as
`source_credentials`, taken when you built it. Logging in again refreshes the
credentials gcloud manages for you; it does not touch that copy, which stays
stale, so gcsdrop will keep failing the same way even after you log in.

Fix it in two steps:

1. Log in again:

       gcloud auth application-default login

2. Regenerate {path} from those fresh credentials.

That file is not a service account key — it holds no private key, only a
refresh token and the service account to impersonate. This project's own
README has the exact command that rebuilds it: see \"Local development\" >
\"Recommended: a scoped credentials file\"."
                )
            }
            // Well-known gcloud ADC file, or no ADC file classified at
            // all (the metadata-server case, where this error should not
            // occur in practice) — either way, plain user credentials
            // with nothing else wrapping them.
            _ => "\
Your Application Default Credentials have expired, and Google is asking you to
log in again.

This is Google's periodic re-authentication ('reauth') policy — the error above
shows \"invalid_rapt\" and/or \"invalid_grant\", which mean your organization
requires it, not that gcsdrop or your setup is broken.

Fix it:

    gcloud auth application-default login

That's it. Plain user credentials don't wrap a separate copy of anything, so
logging in again is the whole fix."
                .to_string(),
        });
    }

    if text.contains("signblob") && text.contains("403") {
        return Some(
            "\
The service account is not allowed to sign on its own behalf.

Grant it roles/iam.serviceAccountTokenCreator on itself:

    gcloud iam service-accounts add-iam-policy-binding SA@PROJECT.iam.gserviceaccount.com \\
      --member='serviceAccount:SA@PROJECT.iam.gserviceaccount.com' \\
      --role='roles/iam.serviceAccountTokenCreator'

Yes, on itself. Signing goes through the IAM signBlob API, where the caller
and the target are the same identity."
                .to_string(),
        );
    }

    // "failed to upload" is the exact context upload.rs attaches
    // (`with_context(|| format!("failed to upload {}", e.relative))`), not
    // the bare word "upload" — which also appears in unrelated messages,
    // including the verify-link failure text below ("the upload succeeded
    // and a signed URL was produced, but ... returned 403 Forbidden"). That
    // bare match used to fire on this function's OWN verify-failure output,
    // appending workspace-write advice underneath a read-permission
    // diagnosis for the same run. Routing verify failures through
    // `RunError::Verify` in main.rs now keeps them out of this function
    // entirely, but the match is narrowed here too, on the theory that a
    // loose substring check will find a new way to double-fire otherwise.
    if text.contains("failed to upload") && text.contains("403") {
        return Some(format!(
            "\
This identity cannot write to the bucket.

Grant it roles/storage.objectCreator ON THE BUCKET, not on the project:

    gcloud storage buckets add-iam-policy-binding gs://{bucket} \\
      --member='serviceAccount:SA@PROJECT.iam.gserviceaccount.com' \\
      --role='roles/storage.objectCreator'

Binding at the project level would grant write access to every bucket in it.

While you're granting bucket IAM: objectCreator alone lets this identity upload but not read,
so it will produce signed URLs that upload fine and then 403 for everyone you share them with.
Grant roles/storage.objectViewer on the same bucket too — gcsdrop checks for this and will tell
you plainly if it's still missing once the upload succeeds."
        ));
    }

    None
}

/// Renders an error chain for display, collapsing layers that repeat text
/// already shown by an earlier layer.
///
/// `anyhow`'s `{:#}` prints every layer in `err.chain()` in turn. That is
/// normally fine, but at least one crate in our dependency chain
/// (google-cloud-auth / google-cloud-gax, for authentication failures)
/// builds an error whose own `Display` already writes its source's full
/// message inline (`"cannot create the authentication headers {source}"`),
/// while ALSO exposing that same source through `std::error::Error::source`
/// — so `{:#}` prints the source's text once as part of the parent's line,
/// then again as the source's own entry. Auth failures in this dependency
/// chain wrap that pattern multiple levels deep (fetching a token to sign
/// a URL needs a token, which itself needs a token, ...), so the same
/// sentence can repeat three or four times before reaching the one useful
/// line — e.g. the `invalid_rapt` credential-expiry case this function
/// exists to fix. See errors::tests for a real captured example.
///
/// The fix does not need to know which crate did this or why: a layer
/// whose full text is already a trailing substring of some earlier KEPT
/// layer's text adds nothing new, so it is dropped. This has to check
/// every earlier kept layer, not just the immediately preceding one — the
/// real credential-expiry chain interleaves an unrelated "failed to
/// generate signature via IAM API" layer between two copies of the same
/// wrapped auth-header text, so the repeat is not always adjacent. Chain
/// order always goes outer (longer, embedding) before inner (shorter,
/// embedded) — that is what "wraps" means — so checking new-against-
/// earlier and not the reverse is enough. Plain `anyhow` `.context()`
/// chains (the common case elsewhere in this codebase) have no overlap
/// between layers at all, so they render exactly as `{:#}` would.
pub fn render(err: &anyhow::Error) -> String {
    let mut out = String::new();
    let mut kept: Vec<String> = Vec::new();
    for cause in err.chain() {
        let text = cause.to_string();
        if !text.is_empty() && kept.iter().any(|k| k.ends_with(text.as_str())) {
            continue; // fully repeated by a layer already printed
        }
        if !out.is_empty() {
            out.push_str(": ");
        }
        out.push_str(&text);
        kept.push(text);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn authorized_user_advice_names_the_gcloud_fix() {
        let msg = cannot_sign(&AdcKind::AuthorizedUser);
        assert!(msg.contains("--impersonate-service-account"), "{msg}");
        assert!(msg.contains("serviceAccountTokenCreator"), "{msg}");
    }

    #[test]
    fn authorized_user_advice_warns_before_overwriting_adc() {
        // That login is machine-wide: it replaces this machine's ADC for
        // every tool that reads them, not just gcsdrop. Silently sending
        // someone into it with no backup step is how they lose their own
        // working credentials.
        let msg = cannot_sign(&AdcKind::AuthorizedUser);
        assert!(msg.contains("overwrites"), "{msg}");
        assert!(
            msg.contains("cp ~/.config/gcloud/application_default_credentials.json"),
            "{msg}"
        );
    }

    #[test]
    fn external_account_advice_differs_from_authorized_user() {
        let a = cannot_sign(&AdcKind::AuthorizedUser);
        let b = cannot_sign(&AdcKind::ExternalAccount);
        assert_ne!(a, b);
        assert!(b.contains("Workload Identity Federation"), "{b}");
    }

    #[test]
    fn external_account_advice_does_not_suggest_a_bare_login() {
        // google-cloud-auth 1.16's build_signer() rejects `external_account`
        // outright (src/credentials.rs:842-844), regardless of what fields
        // are set inside that credential — so a plain re-login can never
        // fix this, and the advice must not imply otherwise.
        let msg = cannot_sign(&AdcKind::ExternalAccount);
        assert!(
            !msg.contains("gcloud auth application-default login \\"),
            "must not suggest re-running the interactive login: {msg}"
        );
        assert!(msg.contains("impersonated_service_account"), "{msg}");
        assert!(msg.contains("source_credentials"), "{msg}");
    }

    #[test]
    fn sign_blob_403_gets_the_self_grant_recipe() {
        let err = anyhow::anyhow!("iamcredentials signBlob returned 403 PERMISSION_DENIED");
        let msg = explain(&err, "my-bucket", None).expect("should be recognised");
        assert!(msg.contains("serviceAccountTokenCreator"), "{msg}");
        assert!(msg.contains("on itself"), "{msg}");
    }

    #[test]
    fn upload_403_gets_the_bucket_binding_recipe() {
        let err = anyhow::anyhow!("failed to upload index.html: 403 storage.objects.create denied");
        let msg = explain(&err, "my-bucket", None).expect("should be recognised");
        assert!(msg.contains("storage.objectCreator"), "{msg}");
        assert!(msg.contains("buckets add-iam-policy-binding"), "{msg}");
        assert!(
            msg.contains("gs://my-bucket"),
            "must name the real bucket: {msg}"
        );
    }

    #[test]
    fn upload_403_advice_also_names_object_viewer() {
        // objectCreator fixes THIS failure (can't write), but a signer with
        // only objectCreator will go on to produce signed URLs that 403 for
        // everyone they're shared with. Telling the reader about
        // objectViewer here, while they're already granting bucket IAM,
        // saves them a second round trip through this same troubleshooting
        // flow.
        let err = anyhow::anyhow!("failed to upload index.html: 403 storage.objects.create denied");
        let msg = explain(&err, "my-bucket", None).expect("should be recognised");
        assert!(msg.contains("storage.objectViewer"), "{msg}");
    }

    #[test]
    fn unrecognised_errors_get_no_advice() {
        let err = anyhow::anyhow!("connection reset by peer");
        assert_eq!(explain(&err, "my-bucket", None), None);
    }

    #[test]
    fn verify_failure_text_does_not_trigger_the_upload_403_advice() {
        // Regression: this shipped once. verify::explain_failure()'s 403
        // message says "the upload succeeded ... returned 403 Forbidden" —
        // which used to satisfy this function's old `contains("upload") &&
        // contains("403")` check, so a link that failed for lack of
        // objectViewer got the unrelated "grant objectCreator" advice
        // printed underneath it, contradicting the diagnosis right above
        // it (the upload HAD succeeded). Feed the real generated text
        // through, not a hand-written stand-in, so wording changes to
        // either function re-run this check.
        let verify_msg = crate::verify::explain_failure(403, None, "my-bucket");
        let err = anyhow::anyhow!(verify_msg);
        assert_eq!(
            explain(&err, "my-bucket", None),
            None,
            "verify's own 403 message must not also get the upload-403 advice appended"
        );
    }

    #[test]
    fn missing_credentials_advice_never_tells_you_to_repoint_the_variable() {
        let msg = cannot_sign(&AdcKind::MissingCredentialsFile("/nope/x.json".into()));
        // The broken version of this message said
        //   export GOOGLE_APPLICATION_CREDENTIALS=/path/to/credentials.json
        // followed by `gcloud auth application-default login`, which writes to
        // the well-known path and never to the variable's path. Following it
        // literally left the reader stuck, with a service account key file as
        // the only way out — and this project forbids those.
        assert!(
            !msg.contains("export GOOGLE_APPLICATION_CREDENTIALS="),
            "must not tell the reader to repoint the variable: {msg}"
        );
        assert!(
            msg.contains("/nope/x.json"),
            "must name the offending path: {msg}"
        );
    }

    // A real `invalid_rapt` failure, captured verbatim from a user's
    // terminal (impersonated_service_account ADC whose source_credentials
    // had aged out). Deliberately NOT shortened: the six-times-wrapped
    // repetition is exactly what the `contains("invalid_rapt")` /
    // `contains("invalid_grant")` matching below has to survive, and what
    // `render()`'s dedup (tested further down) has to collapse.
    const REAL_INVALID_RAPT_ERROR: &str = "failed to sign a URL for gcsdrop/20260907-022446-u86t51co/20260906_issue-471_API金鑰代管驗收.html: signing failed: failed to generate signature via IAM API: cannot create the authentication headers Request to fetch the token failed. Subsequent calls with this credential will also fail. and future attempts will not succeed: failed to generate signature via IAM API: cannot create the authentication headers Request to fetch the token failed. Subsequent calls with this credential will also fail. and future attempts will not succeed: cannot create the authentication headers Request to fetch the token failed. Subsequent calls with this credential will also fail. and future attempts will not succeed: Request to fetch the token failed. Subsequent calls with this credential will also fail. and future attempts will not succeed: cannot create the authentication headers Request to fetch the token failed. Subsequent calls with this credential will also fail. and future attempts will not succeed: Request to fetch the token failed. Subsequent calls with this credential will also fail. and future attempts will not succeed: cannot create the authentication headers failed to refresh user access token, body=<{
  \"error\": \"invalid_grant\",
  \"error_description\": \"reauth related error (invalid_rapt)\",
  \"error_uri\": \"https://support.google.com/a/answer/9368756\",
  \"error_subtype\": \"invalid_rapt\"
}> and future attempts will not succeed: failed to refresh user access token, body=<{
  \"error\": \"invalid_grant\",
  \"error_description\": \"reauth related error (invalid_rapt)\",
  \"error_uri\": \"https://support.google.com/a/answer/9368756\",
  \"error_subtype\": \"invalid_rapt\"
}> and future attempts will not succeed: HTTP status client error (400 Bad Request) for url (https://oauth2.googleapis.com/token)";

    #[test]
    fn credential_expiry_with_plain_user_creds_says_just_log_in_again() {
        let err = anyhow::anyhow!(REAL_INVALID_RAPT_ERROR);
        let well_known = AdcSource::WellKnown(
            "/home/u/.config/gcloud/application_default_credentials.json".into(),
        );
        let msg = explain(&err, "my-bucket", Some(&well_known)).expect("should be recognised");
        assert!(
            msg.contains("gcloud auth application-default login"),
            "{msg}"
        );
        // Nothing else to regenerate for plain user creds — must not send
        // the reader chasing a file that doesn't need rebuilding.
        assert!(!msg.contains("Regenerate"), "{msg}");
    }

    #[test]
    fn credential_expiry_with_no_adc_source_falls_back_to_plain_login_advice() {
        // No classified source (the metadata-server case) shouldn't happen
        // for this error in practice, but must still produce something
        // sane rather than panicking or guessing at a file to rebuild.
        let err = anyhow::anyhow!(REAL_INVALID_RAPT_ERROR);
        let msg = explain(&err, "my-bucket", None).expect("should be recognised");
        assert!(
            msg.contains("gcloud auth application-default login"),
            "{msg}"
        );
    }

    #[test]
    fn credential_expiry_with_impersonation_file_names_the_path_and_readme() {
        let err = anyhow::anyhow!(REAL_INVALID_RAPT_ERROR);
        let from_env = AdcSource::FromEnv("/home/u/.config/gcsdrop/adc.json".into());
        let msg = explain(&err, "my-bucket", Some(&from_env)).expect("should be recognised");
        assert!(
            msg.contains("/home/u/.config/gcsdrop/adc.json"),
            "must name the actual file, not a generic placeholder: {msg}"
        );
        assert!(
            msg.contains("NOT enough"),
            "must say logging in again is not sufficient: {msg}"
        );
        assert!(msg.contains("source_credentials"), "{msg}");
        assert!(
            msg.contains("gcloud auth application-default login"),
            "logging in again is still step 1, just not the whole fix: {msg}"
        );
        assert!(
            msg.to_lowercase().contains("scoped credentials file"),
            "must point at the README section that has the real rebuild command: {msg}"
        );
        // This project forbids service account key files; the advice must
        // say plainly that the impersonation file is not one.
        assert!(
            msg.contains("That file is not a service account key"),
            "{msg}"
        );
    }

    #[test]
    fn credential_expiry_advice_names_it_as_a_reauth_policy_not_a_bug() {
        // A user who thinks the tool is broken debugs the wrong thing.
        // invalid_rapt specifically means the org requires periodic
        // re-authentication — that has to be said plainly.
        let err = anyhow::anyhow!(REAL_INVALID_RAPT_ERROR);
        let msg = explain(&err, "my-bucket", None).expect("should be recognised");
        assert!(msg.to_lowercase().contains("re-authentication"), "{msg}");
        assert!(!msg.to_lowercase().contains("bug"), "{msg}");
    }

    #[test]
    fn credential_expiry_also_matches_bare_invalid_grant() {
        // invalid_grant is the broader OAuth "this refresh token no longer
        // works" error; invalid_rapt is one specific cause of it. Both must
        // land on this same advice, not just the reauth-specific one.
        let err = anyhow::anyhow!(
            "failed to refresh user access token, body=<{{\"error\": \"invalid_grant\", \"error_description\": \"Token has been expired or revoked.\"}}>"
        );
        let msg = explain(&err, "my-bucket", None).expect("should be recognised");
        assert!(
            msg.contains("gcloud auth application-default login"),
            "{msg}"
        );
    }

    #[test]
    fn render_reproduces_the_real_message_from_a_plain_context_chain() {
        // Sanity check on the fixture used below: if this assertion fails,
        // build_real_chain_error() no longer matches the real bug report
        // and the render() tests are no longer testing the real thing.
        let err = build_real_chain_error();
        assert_eq!(format!("{err:#}"), REAL_INVALID_RAPT_ERROR);
    }

    #[test]
    fn render_collapses_the_repeated_auth_failure_sentence() {
        let err = build_real_chain_error();
        let rendered = render(&err);
        let sentence = "Request to fetch the token failed. Subsequent calls with this credential will also fail.";
        let occurrences = rendered.matches(sentence).count();
        assert_eq!(
            occurrences, 1,
            "sentence must appear once, not repeated 4x: {rendered}"
        );
    }

    #[test]
    fn render_keeps_the_top_context_and_the_real_root_cause() {
        let err = build_real_chain_error();
        let rendered = render(&err);
        assert!(rendered.contains("failed to sign a URL for gcsdrop/20260907-022446-u86t51co"));
        assert!(rendered.contains("invalid_rapt"));
        assert!(rendered.contains("HTTP status client error (400 Bad Request)"));
    }

    #[test]
    fn render_is_shorter_than_the_naive_alternate_format() {
        let err = build_real_chain_error();
        assert!(render(&err).len() < format!("{err:#}").len());
    }

    #[test]
    fn render_matches_plain_alternate_format_when_nothing_repeats() {
        // Most error paths in this codebase are plain anyhow .context()
        // chains with no overlap between layers — render() must not touch
        // those at all.
        let err = anyhow::anyhow!("root cause")
            .context("middle")
            .context("top");
        assert_eq!(render(&err), format!("{err:#}"));
    }

    /// Reconstructs the exact chain of error layers behind
    /// `REAL_INVALID_RAPT_ERROR`, using plain `anyhow` `.context()` calls.
    ///
    /// google-cloud-auth's real error types (`CredentialsError`,
    /// `google_cloud_gax::error::Error`) aren't reachable from here to
    /// build a live one, so this reproduces their OBSERVED behavior
    /// instead: an auth-header wrapper whose `Display` bakes in its
    /// source's full text (`"cannot create the authentication headers
    /// {source}"`), stacked several levels deep because signing a URL
    /// needs a token, which itself needs a token from source_credentials,
    /// which itself needs an HTTP call that can fail. Layer order and text
    /// were extracted directly from `REAL_INVALID_RAPT_ERROR` (see
    /// `render_reproduces_the_real_message_from_a_plain_context_chain`
    /// above, which checks this reconstruction is faithful).
    fn build_real_chain_error() -> anyhow::Error {
        const D: &str = "Request to fetch the token failed. Subsequent calls with this credential will also fail. and future attempts will not succeed";
        let json_body = "{\n  \"error\": \"invalid_grant\",\n  \"error_description\": \"reauth related error (invalid_rapt)\",\n  \"error_uri\": \"https://support.google.com/a/answer/9368756\",\n  \"error_subtype\": \"invalid_rapt\"\n}";
        let f = format!(
            "failed to refresh user access token, body=<{json_body}> and future attempts will not succeed"
        );
        let auth_wrap_d = format!("cannot create the authentication headers {D}");
        let auth_wrap_f = format!("cannot create the authentication headers {f}");
        const C: &str = "failed to generate signature via IAM API";

        let e = anyhow::anyhow!(
            "HTTP status client error (400 Bad Request) for url (https://oauth2.googleapis.com/token)"
        );
        let e = e.context(f);
        let e = e.context(auth_wrap_f);
        let e = e.context(D.to_string());
        let e = e.context(auth_wrap_d.clone());
        let e = e.context(D.to_string());
        let e = e.context(auth_wrap_d.clone());
        let e = e.context(auth_wrap_d.clone());
        let e = e.context(C.to_string());
        let e = e.context(auth_wrap_d);
        let e = e.context(C.to_string());
        let e = e.context("signing failed".to_string());
        e.context(
            "failed to sign a URL for gcsdrop/20260907-022446-u86t51co/20260906_issue-471_API金鑰代管驗收.html".to_string(),
        )
    }
}
