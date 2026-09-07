//! Tests that need a real bucket. All #[ignore]d; CI never runs them.
//!
//! Run with:
//!   GCSDROP_BUCKET=<bucket> cargo test --test e2e -- --ignored --nocapture

use google_cloud_storage::client::Storage;

fn bucket() -> String {
    std::env::var("GCSDROP_BUCKET").expect("set GCSDROP_BUCKET to a bucket you can write to")
}

#[tokio::test]
#[ignore = "needs a real bucket and ADC"]
async fn upload_html_succeeds() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let f = dir.path().join("index.html");
    std::fs::write(
        &f,
        "<!doctype html><meta charset=utf-8><h1>gcsdrop works</h1>",
    )?;

    let m = gcsdrop::manifest::scan(dir.path())?;
    let run_id = gcsdrop::manifest::run_id(chrono::Utc::now(), &gcsdrop::manifest::random_suffix());
    let client = Storage::builder().build().await?;
    let bucket = bucket();

    let names = gcsdrop::upload::upload(&client, &bucket, "gcsdrop-test", &run_id, &m).await?;
    assert_eq!(names.len(), 1);
    println!("\nUploaded object: {}\n", names[0]);
    Ok(())
}

#[tokio::test]
#[ignore = "needs a real bucket and signing-capable ADC"]
async fn sign_url_for_uploaded_object() -> anyhow::Result<()> {
    let kind = gcsdrop::adc::detect_adc_kind();
    assert!(
        kind.can_sign(),
        "this ADC ({kind:?}) cannot sign. Run:\n  \
         gcloud auth application-default login --impersonate-service-account=<SA>"
    );

    let dir = tempfile::tempdir()?;
    let f = dir.path().join("index.html");
    std::fs::write(&f, "<!doctype html><meta charset=utf-8><h1>signed</h1>")?;

    let m = gcsdrop::manifest::scan(dir.path())?;
    let run_id = gcsdrop::manifest::run_id(chrono::Utc::now(), &gcsdrop::manifest::random_suffix());
    let client = Storage::builder().build().await?;
    let bucket = bucket();
    let names = gcsdrop::upload::upload(&client, &bucket, "gcsdrop-test", &run_id, &m).await?;

    let signer = gcsdrop::adc::build_signer()?;
    let url = gcsdrop::link::signed_url(
        &signer,
        &bucket,
        &names[0],
        std::time::Duration::from_secs(3600),
    )
    .await?;

    assert!(url.starts_with("https://storage.googleapis.com/"));
    assert!(url.contains("X-Goog-Signature="));
    println!("\nSigned URL (1 hour):\n{url}\n");
    Ok(())
}

#[tokio::test]
#[ignore = "needs a real bucket, signing-capable ADC, and network egress"]
async fn verify_probe_succeeds_for_a_signed_url_the_signer_can_read() -> anyhow::Result<()> {
    // This is the exact regression that shipped once: a signer that can
    // create objects but lacks storage.objects.get produces a URL that
    // signs fine and 403s for everyone it is shared with. If the signer
    // used to run this test lacks roles/storage.objectViewer, this fails
    // here with the same 403 a real recipient would hit.
    let kind = gcsdrop::adc::detect_adc_kind();
    assert!(
        kind.can_sign(),
        "this ADC ({kind:?}) cannot sign. Run:\n  \
         gcloud auth application-default login --impersonate-service-account=<SA>"
    );

    let dir = tempfile::tempdir()?;
    let f = dir.path().join("index.html");
    std::fs::write(&f, "<!doctype html><meta charset=utf-8><h1>verified</h1>")?;

    let m = gcsdrop::manifest::scan(dir.path())?;
    let run_id = gcsdrop::manifest::run_id(chrono::Utc::now(), &gcsdrop::manifest::random_suffix());
    let client = Storage::builder().build().await?;
    let bucket = bucket();
    let names = gcsdrop::upload::upload(&client, &bucket, "gcsdrop-test", &run_id, &m).await?;

    let signer = gcsdrop::adc::build_signer()?;
    let url = gcsdrop::link::signed_url(
        &signer,
        &bucket,
        &names[0],
        std::time::Duration::from_secs(3600),
    )
    .await?;

    let probe = gcsdrop::verify::probe(&url).await?;
    assert!(
        probe.is_ok(),
        "signed URL should be readable with no credentials, got status {} — {}",
        probe.status,
        gcsdrop::verify::explain_failure(probe.status, probe.body.as_deref(), &bucket)
    );
    Ok(())
}
