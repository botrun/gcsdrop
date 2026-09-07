use anyhow::{Context, Result};
use google_cloud_auth::signer::Signer;
use google_cloud_storage::builder::storage::SignedUrlBuilder;
use google_cloud_storage::http::Method;
use std::time::Duration;

/// Produces a V4 signed URL. Anyone holding it can read the object until
/// it expires; it carries no identity check.
///
/// Signing always goes through the IAM signBlob API unless the credentials
/// are a service account key file, so the signing identity needs
/// roles/iam.serviceAccountTokenCreator on itself.
pub async fn signed_url(
    signer: &Signer,
    bucket: &str,
    object: &str,
    ttl: Duration,
) -> Result<String> {
    SignedUrlBuilder::for_object(format!("projects/_/buckets/{bucket}"), object)
        .with_method(Method::GET)
        .with_expiration(ttl)
        .sign_with(signer)
        .await
        .with_context(|| format!("failed to sign a URL for {object}"))
}
