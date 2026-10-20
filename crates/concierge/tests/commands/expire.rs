use std::process::{Command, Output};

use concierge::{db, name::MemberName, upload_rules::RETENTION};

use crate::Fixture;

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
    let ids = ["AAAAAAAAAAAAAAAAAAAAAA", "BBBBBBBBBBBBBBBBBBBBBB"];
    for (id, created_at) in ids.iter().zip([old, db::now()]) {
        let upload = fixture.config.uploads.directory.join(format!("{id}.jpg"));
        std::fs::write(upload, "a scrubbed photo").unwrap();
        fixture
            .execute(&format!(
                "INSERT INTO uploads (id, account, owner, content_type, size, created_at)
                 VALUES ('{id}', 'alice', 'alice', 'image/jpeg', 16, {created_at})"
            ))
            .await;
    }

    let output = expire(&fixture);

    assert!(output.status.success(), "{output:?}");
    let log = String::from_utf8(output.stderr).unwrap();
    assert_eq!(
        log,
        "Deleted 1 uploads past their days and 0 files without one\n"
    );
    assert_eq!(fixture.scrubber.stored(), ["BBBBBBBBBBBBBBBBBBBBBB.jpg"]);
    let left = fixture
        .db()
        .await
        .uploads_of(&MemberName::new("alice"))
        .await
        .unwrap();
    assert_eq!(left.len(), 1);
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
