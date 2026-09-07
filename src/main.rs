use anyhow::{bail, Context, Result};
use clap::Parser;
use gcsdrop::{adc, config, errors, link, manifest, upload, verify};
use std::path::PathBuf;
use std::time::Duration;

/// V4 signed URLs cannot outlive this. Google's limit, not ours.
const MAX_EXPIRES: Duration = Duration::from_secs(604_800);

#[derive(Parser, Debug)]
#[command(
    name = "gcsdrop",
    version,
    about = "Publish HTML to your own GCS bucket and get a shareable URL"
)]
pub struct Cli {
    /// File or directory to publish
    pub path: PathBuf,

    /// Signed URL lifetime, e.g. 30m, 1h, 7d. Maximum 7d. Defaults to 1h.
    #[arg(long)]
    pub expires: Option<String>,
}

pub fn parse_duration(s: &str) -> Result<Duration> {
    let s = s.trim();
    let mut chars = s.chars();
    let unit = chars.next_back().unwrap_or(' ');
    let num = chars.as_str();
    let n: u64 = num
        .parse()
        .map_err(|_| anyhow::anyhow!("cannot read '{s}' as a duration; try 30m, 1h or 7d"))?;
    let secs = match unit {
        's' => n,
        'm' => n * 60,
        'h' => n * 3600,
        'd' => n * 86400,
        _ => bail!("cannot read '{s}' as a duration; try 30m, 1h or 7d"),
    };
    if secs == 0 {
        bail!("a signed URL must last longer than zero seconds");
    }
    let d = Duration::from_secs(secs);
    if d > MAX_EXPIRES {
        bail!("'{s}' is longer than 7 days. Google caps V4 signed URLs at 604800 seconds.");
    }
    Ok(d)
}

pub fn expiry_from(cli: &Cli) -> Result<Duration> {
    match &cli.expires {
        Some(s) => parse_duration(s),
        None => Ok(Duration::from_secs(3600)),
    }
}

/// `run()`'s error type. Kept separate from a bare `anyhow::Error` so the
/// verify-link failure — whose message is already complete, built by
/// `verify::explain_failure()` — cannot be re-scanned by
/// `errors::explain()`'s keyword matching on the way out. It was, once:
/// that message contains both "upload" and "403" (it says the upload
/// succeeded and the link then 403'd), which used to trip the unrelated
/// upload-403 advice and print two contradictory diagnoses for one
/// failure. Routing it through its own variant, handled in `main()` before
/// `explain()` is ever called, makes that impossible rather than just
/// avoided for today's wording.
enum RunError {
    /// A complete, ready-to-print message. No further advice is appended.
    Verify(String),
    /// Everything else — still eligible for `errors::explain()`'s advice.
    Other(anyhow::Error),
}

impl From<anyhow::Error> for RunError {
    fn from(e: anyhow::Error) -> Self {
        RunError::Other(e)
    }
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    // Parsed here, rather than inside run(), so a later error can still
    // name the real bucket in its advice.
    let cfg = match config::from_env() {
        Ok(cfg) => cfg,
        Err(e) => {
            eprintln!("Error: {}", errors::render(&e));
            std::process::exit(1);
        }
    };

    if let Err(e) = run(&cli, &cfg).await {
        match e {
            RunError::Verify(msg) => eprintln!("Error: {msg}"),
            RunError::Other(e) => {
                eprintln!("Error: {}", errors::render(&e));
                if let Some(advice) = errors::explain(&e, &cfg.bucket, adc::adc_source().as_ref()) {
                    eprintln!("\n{advice}");
                }
            }
        }
        std::process::exit(1);
    }
}

async fn run(cli: &Cli, cfg: &config::Config) -> Result<(), RunError> {
    let ttl = expiry_from(cli)?;

    // Local checks that can fail come before any irreversible remote side
    // effect: scanning the filesystem is free and needs no network.
    let m = manifest::scan(&cli.path)?;

    // Check signing capability before touching the network: finding out
    // only after the bytes are already in the bucket leaves an orphaned
    // object with no URL to reach it.
    let kind = adc::detect_adc_kind();
    if !kind.can_sign() {
        return Err(anyhow::anyhow!("{}", errors::cannot_sign(&kind)).into());
    }

    let run_id = manifest::run_id(chrono::Utc::now(), &manifest::random_suffix());
    // The manifest already determines the landing object's path; no need to
    // wait for the upload's return value to know it.
    let landing = match &m.index {
        Some(i) => upload::object_path(&cfg.prefix, &run_id, i),
        None => upload::object_path(&cfg.prefix, &run_id, &m.entries[0].relative),
    };

    // Sign before uploading anything: signing never touches the object, so
    // a signBlob failure here (e.g. the self-grant is missing) still leaves
    // the bucket untouched.
    let signer = adc::build_signer()?;
    let url = link::signed_url(&signer, &cfg.bucket, &landing, ttl).await?;

    // The first irreversible write of content.
    let client = google_cloud_storage::client::Storage::builder()
        .build()
        .await
        .context("failed to build a Cloud Storage client")?;

    eprintln!("Uploading {} file(s)...", m.entries.len());
    // The return value now only matters for verification, not for building
    // `url` (computed above, before the upload). Kept, not discarded.
    let _names = upload::upload(&client, &cfg.bucket, &cfg.prefix, &run_id, &m).await?;

    // A valid signature is not enough: GCS authorizes a V4 signed URL as the
    // *signer*, so the signer needs every permission the URL exercises. The
    // signer here could hold storage.objectCreator but not
    // storage.objects.get, in which case every GET signed URL it produces
    // 403s for anyone we hand it to. Fetch it ourselves, with no
    // credentials, exactly as the recipient would, rather than print a link
    // we have not confirmed works.
    eprintln!("Verifying the link works...");
    let probe = verify::probe(&url).await?;
    if !probe.is_ok() {
        return Err(RunError::Verify(verify::explain_failure(
            probe.status,
            probe.body.as_deref(),
            &cfg.bucket,
        )));
    }

    println!("{url}");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[test]
    fn parse_duration_accepts_suffixes() {
        assert_eq!(parse_duration("30m").unwrap(), Duration::from_secs(1800));
        assert_eq!(parse_duration("1h").unwrap(), Duration::from_secs(3600));
        assert_eq!(parse_duration("7d").unwrap(), Duration::from_secs(604800));
        assert_eq!(parse_duration("90s").unwrap(), Duration::from_secs(90));
    }

    #[test]
    fn parse_duration_rejects_over_seven_days() {
        let err = parse_duration("8d").unwrap_err().to_string();
        assert!(err.contains("7 days"), "got: {err}");
    }

    #[test]
    fn parse_duration_rejects_zero() {
        assert!(parse_duration("0s").is_err());
    }

    #[test]
    fn parse_duration_rejects_garbage() {
        assert!(parse_duration("soon").is_err());
        assert!(parse_duration("10").is_err());
        assert!(parse_duration("").is_err());
    }

    #[test]
    fn default_is_one_hour_signed_url() {
        let cli = Cli::parse_from(["gcsdrop", "./report.html"]);
        assert_eq!(expiry_from(&cli).unwrap(), Duration::from_secs(3600));
    }

    #[test]
    fn expires_flag_is_honoured() {
        let cli = Cli::parse_from(["gcsdrop", "./x.html", "--expires", "7d"]);
        assert_eq!(expiry_from(&cli).unwrap(), Duration::from_secs(604800));
    }

    #[test]
    fn parse_duration_rejects_multibyte_without_panicking() {
        assert!(parse_duration("1天").is_err());
        assert!(parse_duration("7天").is_err());
        assert!(parse_duration("秒").is_err());
    }
}
