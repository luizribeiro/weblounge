use std::process::{Command, Output};

use concierge::{db, upload_rules::RETENTION};

use crate::Fixture;

const OLD: &str = "AAAAAAAAAAAAAAAAAAAAAA";
const NEW: &str = "BBBBBBBBBBBBBBBBBBBBBB";

/// `concierge expire-uploads` with only what its unit sets.
fn expire(fixture: &Fixture) -> Output {
    Command::new(env!("CARGO_BIN_EXE_concierge"))
        .arg("expire-uploads")
        .env_clear()
        .env("STATE_DIRECTORY", fixture.state_dir())
        .env("CONCIERGE_UPLOADS", &fixture.config.uploads.directory)
        .env("CONCIERGE_SCRUBBER", &fixture.config.uploads.scrubber)
        .output()
        .unwrap()
}

#[tokio::test]
async fn deletes_the_uploads_past_their_days_and_logs_how_many() {
    let fixture = Fixture::start().await;
    fixture.insert_member("alice", None).await;
    let old = db::now() - RETENTION - 1;
    fixture.add_upload(OLD, "alice", "alice", old).await;
    fixture.add_upload(NEW, "alice", "alice", db::now()).await;

    let output = expire(&fixture);

    assert!(output.status.success(), "{output:?}");
    let log = String::from_utf8(output.stderr).unwrap();
    assert_eq!(
        log,
        "Deleted 1 uploads past their days and 0 files without one\n"
    );
    let left = vec![NEW.to_owned()];
    assert_eq!(fixture.uploads_left().await, (left.clone(), left));
}

#[test]
fn needs_the_uploads_directory() {
    let output = Command::new(env!("CARGO_BIN_EXE_concierge"))
        .arg("expire-uploads")
        .env_clear()
        .env("STATE_DIRECTORY", "/nonexistent")
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("CONCIERGE_UPLOADS is not set"), "{stderr}");
}
