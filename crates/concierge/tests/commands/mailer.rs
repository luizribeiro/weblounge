use std::{
    fs,
    io::{BufRead, BufReader},
    process::{Child, Command, Stdio},
    sync::mpsc,
    time::Duration,
};

use concierge::{
    config::SMTP_PASSWORD_CREDENTIAL,
    db,
    email::Email,
    outbox::{Message, Subject},
};

use crate::Fixture;

const LOG_TIMEOUT: Duration = Duration::from_secs(30);

fn write_smtp_password(fixture: &Fixture) {
    let path = fixture
        .config
        .credentials_dir
        .join(SMTP_PASSWORD_CREDENTIAL);
    fs::write(path, "correct horse battery staple\n").unwrap();
}

/// `concierge mailer` with only the mailer's environment, sending to a
/// port nothing listens on.
fn start_mailer(fixture: &Fixture) -> Child {
    let closed = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = closed.local_addr().unwrap().port().to_string();
    drop(closed);
    Command::new(env!("CARGO_BIN_EXE_concierge"))
        .arg("mailer")
        .env_clear()
        .env("CONCIERGE_SMTP_HOST", "127.0.0.1")
        .env("CONCIERGE_SMTP_PORT", port)
        .env("CONCIERGE_SMTP_USERNAME", "noreply@weblounge.org")
        .env("CONCIERGE_MAIL_FROM", "weblounge <noreply@weblounge.org>")
        .env("STATE_DIRECTORY", fixture.state_dir())
        .env("CREDENTIALS_DIRECTORY", &fixture.config.credentials_dir)
        .stderr(Stdio::piped())
        .spawn()
        .unwrap()
}

/// What `mailer` logs, up to and including the first line `until` accepts.
fn log_until(mailer: &mut Child, until: impl Fn(&str) -> bool) -> Vec<String> {
    let (lines, receive) = mpsc::channel();
    let stderr = BufReader::new(mailer.stderr.take().unwrap());
    std::thread::spawn(move || {
        for line in stderr.lines() {
            if lines.send(line.unwrap()).is_err() {
                break;
            }
        }
    });
    let mut log = Vec::new();
    while let Ok(line) = receive.recv_timeout(LOG_TIMEOUT) {
        let done = until(&line);
        log.push(line);
        if done {
            return log;
        }
    }
    panic!("the mailer stopped or went quiet: {log:?}");
}

#[tokio::test]
async fn retries_mail_it_cannot_send_and_logs_no_address() {
    let fixture = Fixture::start().await;
    let message = Message {
        account: None,
        to: Email::parse("alice@example.org").unwrap(),
        subject: Subject::parse("Verify your email for weblounge").unwrap(),
        body: "Open https://members.weblounge.org/verify#q7Hx2mZp9vLcT4wRfBa\n".into(),
    };
    fixture
        .db()
        .await
        .queue_message(&message, db::now())
        .await
        .unwrap();
    write_smtp_password(&fixture);
    let mut mailer = start_mailer(&fixture);

    let log = log_until(&mut mailer, |line| line.contains("on attempt 1"));
    mailer.kill().unwrap();
    mailer.wait().unwrap();

    assert!(
        log[0].starts_with("Sending mail through 127.0.0.1:"),
        "{log:?}"
    );
    assert!(
        log[1].starts_with("Sending message 1 failed (Connection error: "),
        "{log:?}"
    );
    let log = log.join("\n");
    for private in ["alice", "Verify", "q7Hx2mZp9vLcT4wRfBa"] {
        assert!(!log.contains(private), "{log}");
    }
    let due = fixture
        .db()
        .await
        .due_messages(db::now() + 60)
        .await
        .unwrap();
    assert_eq!(due[0].attempts, 1);
}

#[tokio::test]
async fn stops_without_the_smtp_password() {
    let fixture = Fixture::start().await;
    let mut mailer = start_mailer(&fixture);

    let log = log_until(&mut mailer, |line| line.starts_with("Error: "));

    assert!(!mailer.wait().unwrap().success());
    assert!(
        log.last().unwrap().contains(SMTP_PASSWORD_CREDENTIAL),
        "{log:?}"
    );
}
