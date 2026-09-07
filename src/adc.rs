use anyhow::{Context, Result};
use google_cloud_auth::signer::Signer;

/// Which flavour of Application Default Credentials is in play.
///
/// This mirrors the branches in google-cloud-auth's `build_signer`
/// (src/auth/src/credentials.rs). We detect it ourselves so we can print
/// advice instead of the crate's "authorized_user signer is not supported".
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AdcKind {
    /// No ADC file; falls back to the GCE/GKE metadata server.
    MetadataServer,
    ServiceAccountKey,
    Impersonated,
    AuthorizedUser,
    ExternalAccount,
    /// GOOGLE_APPLICATION_CREDENTIALS is set but points at a file we cannot
    /// read. Distinct from the well-known file being absent, which
    /// legitimately means "fall back to the metadata server".
    MissingCredentialsFile(std::path::PathBuf),
    Unknown(String),
}

impl AdcKind {
    /// Whether a V4 signed URL can be produced with these credentials.
    pub fn can_sign(&self) -> bool {
        matches!(
            self,
            AdcKind::MetadataServer | AdcKind::ServiceAccountKey | AdcKind::Impersonated
        )
    }
}

/// Where the ADC file came from. The two cases behave differently when the
/// file is missing, so they must not be collapsed into one: an explicitly
/// set `GOOGLE_APPLICATION_CREDENTIALS` that doesn't exist is a
/// configuration error, while the well-known gcloud file simply not being
/// there means "use the metadata server".
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AdcSource {
    FromEnv(std::path::PathBuf),
    WellKnown(std::path::PathBuf),
}

/// Where to look for an ADC file, if anywhere. `None` means "fall back to
/// the GCE/GKE metadata server" — that is a valid, working configuration,
/// not an error.
pub fn adc_source() -> Option<AdcSource> {
    if let Ok(p) = std::env::var("GOOGLE_APPLICATION_CREDENTIALS") {
        return Some(AdcSource::FromEnv(p.into()));
    }
    let home = std::env::var("HOME").ok()?;
    let p =
        std::path::PathBuf::from(home).join(".config/gcloud/application_default_credentials.json");
    p.exists().then_some(AdcSource::WellKnown(p))
}

pub fn detect_adc_kind() -> AdcKind {
    adc_kind_from_source(adc_source())
}

/// Pure: what ADC kind a given source resolves to. Split out from
/// `detect_adc_kind` so tests can drive the missing-file cases without
/// mutating the process-wide `GOOGLE_APPLICATION_CREDENTIALS`/`HOME`
/// environment variables, which Rust's parallel test runner makes unsafe to
/// touch. Mirrors how `config::from_env` wraps `config::from_vars`.
pub fn adc_kind_from_source(source: Option<AdcSource>) -> AdcKind {
    match source {
        None => AdcKind::MetadataServer,
        Some(AdcSource::FromEnv(p)) => match std::fs::read_to_string(&p) {
            Ok(s) => kind_from_json(&s),
            // Explicitly pointed somewhere unreadable: say so. Falling back
            // silently here would fail later, somewhere much harder to
            // understand (a bare metadata-server error).
            Err(_) => AdcKind::MissingCredentialsFile(p),
        },
        Some(AdcSource::WellKnown(p)) => match std::fs::read_to_string(&p) {
            Ok(s) => kind_from_json(&s),
            Err(_) => AdcKind::MetadataServer,
        },
    }
}

pub fn kind_from_json(s: &str) -> AdcKind {
    let v: serde_json::Value = match serde_json::from_str(s) {
        Ok(v) => v,
        Err(_) => return AdcKind::Unknown("unparseable".into()),
    };
    match v.get("type").and_then(|t| t.as_str()) {
        Some("authorized_user") => AdcKind::AuthorizedUser,
        Some("service_account") => AdcKind::ServiceAccountKey,
        Some("impersonated_service_account") => AdcKind::Impersonated,
        Some("external_account") => AdcKind::ExternalAccount,
        Some(other) => AdcKind::Unknown(other.to_string()),
        None => AdcKind::Unknown("missing type field".into()),
    }
}

pub fn build_signer() -> Result<Signer> {
    google_cloud_auth::credentials::Builder::default()
        .build_signer()
        .context("failed to build a signer from Application Default Credentials")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kind_from_json_reads_the_type_field() {
        let cases = [
            (r#"{"type":"authorized_user"}"#, AdcKind::AuthorizedUser),
            (r#"{"type":"service_account"}"#, AdcKind::ServiceAccountKey),
            (
                r#"{"type":"impersonated_service_account"}"#,
                AdcKind::Impersonated,
            ),
            (r#"{"type":"external_account"}"#, AdcKind::ExternalAccount),
        ];
        for (json, expected) in cases {
            assert_eq!(kind_from_json(json), expected, "json: {json}");
        }
    }

    #[test]
    fn unknown_type_is_reported_verbatim() {
        assert_eq!(
            kind_from_json(r#"{"type":"martian_account"}"#),
            AdcKind::Unknown("martian_account".into())
        );
    }

    #[test]
    fn malformed_json_is_unknown() {
        assert!(matches!(kind_from_json("not json"), AdcKind::Unknown(_)));
    }

    #[test]
    fn signing_kinds_are_classified_correctly() {
        assert!(AdcKind::MetadataServer.can_sign());
        assert!(AdcKind::Impersonated.can_sign());
        assert!(AdcKind::ServiceAccountKey.can_sign());
        assert!(!AdcKind::AuthorizedUser.can_sign());
        assert!(!AdcKind::ExternalAccount.can_sign());
    }

    #[test]
    fn missing_credentials_file_cannot_sign() {
        assert!(!AdcKind::MissingCredentialsFile("/nope/missing.json".into()).can_sign());
    }

    #[test]
    fn env_pointing_at_a_missing_file_is_reported_not_silently_ignored() {
        // GOOGLE_APPLICATION_CREDENTIALS set to a path that doesn't exist is a
        // configuration error, not "no ADC file" — it must not be silently
        // treated the same as the well-known file being absent. Drives the
        // pure classifier directly (not detect_adc_kind()/the real env var),
        // since mutating a process-wide env var is unsafe under the parallel
        // test runner.
        let source = Some(AdcSource::FromEnv("/nope/does-not-exist.json".into()));
        assert!(matches!(
            adc_kind_from_source(source),
            AdcKind::MissingCredentialsFile(_)
        ));
    }

    #[test]
    fn well_known_file_missing_falls_back_to_metadata_server() {
        // The well-known gcloud file simply not existing is normal — it means
        // "use the metadata server", not an error.
        let source = Some(AdcSource::WellKnown("/nope/does-not-exist.json".into()));
        assert_eq!(adc_kind_from_source(source), AdcKind::MetadataServer);
    }
}
