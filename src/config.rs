use anyhow::{bail, Context, Result};

/// Runtime configuration, read from environment variables.
///
/// No GCP project is needed: bucket names are globally unique and the
/// upload API does not take a project.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    pub bucket: String,
    pub prefix: String,
}

pub fn from_env() -> Result<Config> {
    from_vars(
        std::env::var("GCSDROP_BUCKET").ok(),
        std::env::var("GCSDROP_PREFIX").ok(),
    )
    .context("failed to read configuration from the environment")
}

pub fn from_vars(bucket: Option<String>, prefix: Option<String>) -> Result<Config> {
    let bucket = bucket.unwrap_or_default();

    let bucket = bucket
        .trim()
        .trim_start_matches("gs://")
        .trim_matches('/')
        .to_string();

    if bucket.is_empty() {
        bail!(
            "GCSDROP_BUCKET is not set.\n\
             \n\
             Point it at a bucket you own:\n\
             \n\
             \x20   export GCSDROP_BUCKET=my-bucket\n\
             \n\
             gcsdrop never creates buckets. Create one first:\n\
             \n\
             \x20   gcloud storage buckets create gs://my-bucket \\\n\
             \x20     --project=PROJECT --location=LOCATION \\\n\
             \x20     --uniform-bucket-level-access"
        )
    }

    let prefix = prefix
        .unwrap_or_else(|| "gcsdrop".to_string())
        .trim()
        .trim_matches('/')
        .to_string();

    Ok(Config { bucket, prefix })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bucket_is_required() {
        let err = from_vars(None, None).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("GCSDROP_BUCKET"), "got: {msg}");
    }

    #[test]
    fn prefix_defaults_to_gcsdrop() {
        let c = from_vars(Some("my-bucket".into()), None).unwrap();
        assert_eq!(c.bucket, "my-bucket");
        assert_eq!(c.prefix, "gcsdrop");
    }

    #[test]
    fn prefix_can_be_overridden() {
        let c = from_vars(Some("b".into()), Some("reports".into())).unwrap();
        assert_eq!(c.prefix, "reports");
    }

    #[test]
    fn prefix_slashes_are_trimmed() {
        let c = from_vars(Some("b".into()), Some("/reports/".into())).unwrap();
        assert_eq!(c.prefix, "reports");
    }

    #[test]
    fn empty_prefix_is_allowed() {
        let c = from_vars(Some("b".into()), Some("".into())).unwrap();
        assert_eq!(c.prefix, "");
    }

    #[test]
    fn gs_scheme_is_stripped_from_bucket() {
        let c = from_vars(Some("gs://my-bucket".into()), None).unwrap();
        assert_eq!(c.bucket, "my-bucket");
    }

    #[test]
    fn bucket_that_normalizes_to_empty_is_rejected() {
        let err = from_vars(Some("gs://".into()), None)
            .unwrap_err()
            .to_string();
        assert!(err.contains("GCSDROP_BUCKET"), "got: {err}");
    }

    #[test]
    fn bucket_of_only_slashes_is_rejected() {
        let err = from_vars(Some("/".into()), None).unwrap_err().to_string();
        assert!(err.contains("GCSDROP_BUCKET"), "got: {err}");
    }
}
