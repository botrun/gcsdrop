use crate::manifest::Manifest;
use anyhow::{Context, Result};
use google_cloud_storage::client::Storage;

/// Builds the object name inside the bucket.
pub fn object_path(prefix: &str, run_id: &str, relative: &str) -> String {
    let mut parts: Vec<&str> = Vec::new();
    if !prefix.is_empty() {
        parts.push(prefix);
    }
    parts.push(run_id);
    parts.push(relative);
    parts.join("/")
}

/// Uploads every entry. Returns the object names in the same order.
pub async fn upload(
    client: &Storage,
    bucket: &str,
    prefix: &str,
    run_id: &str,
    m: &Manifest,
) -> Result<Vec<String>> {
    let mut names = Vec::with_capacity(m.entries.len());
    for e in &m.entries {
        let object = object_path(prefix, run_id, &e.relative);
        let payload = tokio::fs::File::open(&e.local)
            .await
            .with_context(|| format!("cannot read {}", e.local.display()))?;
        client
            .write_object(format!("projects/_/buckets/{bucket}"), &object, payload)
            .set_content_type(&e.content_type)
            .send_unbuffered()
            .await
            .with_context(|| format!("failed to upload {}", e.relative))?;
        names.push(object);
    }
    Ok(names)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn object_path_joins_with_slashes() {
        assert_eq!(
            object_path("gcsdrop", "20260903-040506-abcd1234", "index.html"),
            "gcsdrop/20260903-040506-abcd1234/index.html"
        );
    }

    #[test]
    fn object_path_keeps_nested_relatives() {
        assert_eq!(
            object_path("reports", "r1", "assets/style.css"),
            "reports/r1/assets/style.css"
        );
    }

    #[test]
    fn object_path_omits_empty_prefix() {
        assert_eq!(object_path("", "r1", "index.html"), "r1/index.html");
    }
}
