use std::process::{Command, Output};

use concierge::db::Db;

/// `concierge migrate` with only `STATE_DIRECTORY` set, as its unit runs it.
fn migrate(state: &tempfile::TempDir) -> Output {
    Command::new(env!("CARGO_BIN_EXE_concierge"))
        .arg("migrate")
        .env_clear()
        .env("STATE_DIRECTORY", state.path())
        .output()
        .unwrap()
}

#[tokio::test]
async fn creates_the_database_the_other_commands_open() {
    let state = tempfile::tempdir().unwrap();
    let path = state.path().join("concierge.db");
    assert!(Db::open(&path).await.is_err());

    let first = migrate(&state);
    let again = migrate(&state);

    let version = Db::open(&path).await.unwrap().migration_version().await;
    let logged = format!(
        "{} is at migration {}\n",
        path.display(),
        version.unwrap().unwrap()
    );
    for output in [first, again] {
        assert!(output.status.success(), "{output:?}");
        assert_eq!(String::from_utf8(output.stderr).unwrap(), logged);
    }
}

#[test]
fn needs_the_state_directory() {
    let output = Command::new(env!("CARGO_BIN_EXE_concierge"))
        .arg("migrate")
        .env_clear()
        .output()
        .unwrap();

    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("STATE_DIRECTORY is not set"), "{stderr}");
}
