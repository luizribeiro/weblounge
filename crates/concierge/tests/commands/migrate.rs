use std::{
    path::Path,
    process::{Command, Output},
};

use concierge::{db::Db, outbox::Outbox};

/// `concierge migrate` with only what its unit sets: the state directory
/// and the outbox.
fn migrate(state: &Path, outbox: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_concierge"))
        .arg("migrate")
        .env_clear()
        .env("STATE_DIRECTORY", state)
        .env("CONCIERGE_OUTBOX", outbox)
        .output()
        .unwrap()
}

#[tokio::test]
async fn creates_both_databases_the_other_commands_open() {
    let (state, shared) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let database = state.path().join("concierge.db");
    let outbox = shared.path().join("outbox.db");
    assert!(Db::open(&database).await.is_err());
    assert!(Outbox::open(&outbox).await.is_err());

    let first = migrate(state.path(), &outbox);
    let again = migrate(state.path(), &outbox);

    let db = Db::open(&database).await.unwrap();
    let opened_outbox = Outbox::open(&outbox).await.unwrap();
    let logged = format!(
        "{} is at migration {}\n{} is at migration {}\n",
        database.display(),
        db.migration_version().await.unwrap().unwrap(),
        outbox.display(),
        opened_outbox.migration_version().await.unwrap().unwrap(),
    );
    for output in [first, again] {
        assert!(output.status.success(), "{output:?}");
        assert_eq!(String::from_utf8(output.stderr).unwrap(), logged);
    }
}

#[test]
fn needs_the_state_directory_and_the_outbox() {
    let output = Command::new(env!("CARGO_BIN_EXE_concierge"))
        .arg("migrate")
        .env_clear()
        .env("STATE_DIRECTORY", "/var/lib/concierge")
        .output()
        .unwrap();

    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("CONCIERGE_OUTBOX is not set"), "{stderr}");
}
