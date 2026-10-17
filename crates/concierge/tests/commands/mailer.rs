use std::{
    fs,
    io::{BufRead, BufReader},
    process::{Child, Command, Stdio},
    sync::mpsc,
    time::Duration,
};

use concierge::{config::SMTP_PASSWORD_CREDENTIAL, db, email::Email};
use outbox::{Message, Subject};

use crate::Fixture;

const LOG_TIMEOUT: Duration = Duration::from_secs(30);

fn write_smtp_password(fixture: &Fixture) {
    let path = fixture
        .config
        .credentials_dir
        .join(SMTP_PASSWORD_CREDENTIAL);
    fs::write(path, "correct horse battery staple\n").unwrap();
}

/// `concierge mailer <args>` with only the mailer's environment, sending
/// to a port nothing listens on.
fn mailer_command(fixture: &Fixture, args: &[&str]) -> Command {
    let closed = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = closed.local_addr().unwrap().port().to_string();
    drop(closed);
    let mut command = Command::new(env!("CARGO_BIN_EXE_concierge"));
    command
        .arg("mailer")
        .args(args)
        .env_clear()
        .env("CONCIERGE_SMTP_HOST", "127.0.0.1")
        .env("CONCIERGE_SMTP_PORT", port)
        .env("CONCIERGE_SMTP_USERNAME", "noreply@weblounge.org")
        .env("CONCIERGE_MAIL_FROM", "weblounge <noreply@weblounge.org>")
        .env("CONCIERGE_OUTBOX", &fixture.config.outbox_path)
        .env("CREDENTIALS_DIRECTORY", &fixture.config.credentials_dir);
    command
}

fn start_mailer(fixture: &Fixture) -> Child {
    let mut command = mailer_command(fixture, &[]);
    command.stderr(Stdio::piped()).spawn().unwrap()
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
        .outbox()
        .await
        .queue(&message, db::now())
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
    let due = fixture.outbox().await.due(db::now() + 60).await.unwrap();
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

#[tokio::test]
async fn checking_names_the_server_it_cannot_log_in_to_and_sends_nothing() {
    let fixture = Fixture::start().await;
    let message = Message {
        account: None,
        to: Email::parse("alice@example.org").unwrap(),
        subject: Subject::parse("Verify your email for weblounge").unwrap(),
        body: "Open https://members.weblounge.org/verify#q7Hx2mZp9vLcT4wRfBa\n".into(),
    };
    let outbox = fixture.outbox().await;
    outbox.queue(&message, db::now()).await.unwrap();
    write_smtp_password(&fixture);

    let output = mailer_command(&fixture, &["--check"]).output().unwrap();

    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(
        stderr.starts_with("Error: Can't log in to 127.0.0.1:"),
        "{stderr}"
    );
    assert!(
        stderr.contains(" as noreply@weblounge.org: Connection error: "),
        "{stderr}"
    );
    assert_eq!(outbox.due(db::now()).await.unwrap()[0].attempts, 0);
}
