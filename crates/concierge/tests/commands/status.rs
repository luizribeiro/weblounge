use std::{fs, os::unix::fs::MetadataExt, os::unix::fs::PermissionsExt};

use concierge::status::report;

use crate::Fixture;

#[tokio::test]
async fn reports_the_url_state_key_database_and_ergo() {
    let fixture = Fixture::start().await;

    let report = fixture.run(&["status"]).await.unwrap();

    let state = fixture.state_dir();
    assert_eq!(
        report,
        format!(
            "public url: https://members.weblounge.localhost:10443\n\
             state directory: {} (writable by uid {})\n\
             session key: 64 bytes\n\
             database: {}/concierge.db (migration 20261101120000, 0 members)\n\
             ergo: {} answers, version 2.19.1-fake\n",
            state.display(),
            fs::metadata(state).unwrap().uid(),
            state.display(),
            fixture.ergo.url(),
        )
    );
}

#[tokio::test]
async fn leaves_only_the_database_in_the_state_directory() {
    let fixture = Fixture::start().await;
    fixture.run(&["status"]).await.unwrap();
    let mut files: Vec<_> = fs::read_dir(fixture.state_dir())
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .filter(|name| !name.starts_with("concierge.db"))
        .collect();
    files.sort();
    assert_eq!(files, Vec::<String>::new());
}

#[tokio::test]
async fn reports_an_ergo_that_does_not_answer() {
    let Fixture {
        config,
        ergo,
        _state,
        _outbox,
        mailer_key: _,
        scrubber: _,
        _credentials,
    } = Fixture::start().await;
    ergo.stop().await;

    let report = report(&config).await.unwrap();

    let ergo_line = report.lines().last().unwrap();
    let expected = format!(
        "ergo: {} does not answer: could not talk to Ergo: ",
        config.ergo_api
    );
    assert!(ergo_line.starts_with(&expected), "{report}");
}

#[tokio::test]
async fn fails_without_the_ergo_api_token() {
    let fixture = Fixture::start().await;
    fs::remove_file(fixture.config.credentials_dir.join("ergo-api-token")).unwrap();
    let error = fixture.run(&["status"]).await.unwrap_err();
    assert!(error.to_string().contains("ergo-api-token"), "{error}");
}

#[tokio::test]
async fn fails_on_a_state_directory_it_cannot_write() {
    let fixture = Fixture::start().await;
    fs::set_permissions(fixture.state_dir(), fs::Permissions::from_mode(0o500)).unwrap();
    let error = fixture.run(&["status"]).await.unwrap_err();
    assert!(error.to_string().contains("is not writable"), "{error}");
}
