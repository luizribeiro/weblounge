use std::process::{Command, Output};

use concierge::{
    db::{self, NewMember},
    name::MemberName,
    verification_mail::REMINDER_SUBJECT,
    verification_rules::REMINDER_AFTER,
};

use crate::Fixture;

/// `concierge remind` with only what its unit sets: the databases and
/// the public URL, without credentials or Ergo.
fn remind(fixture: &Fixture) -> Output {
    Command::new(env!("CARGO_BIN_EXE_concierge"))
        .arg("remind")
        .env_clear()
        .env("STATE_DIRECTORY", fixture.state_dir())
        .env("CONCIERGE_OUTBOX", &fixture.config.outbox_path)
        .env("CONCIERGE_PUBLIC_URL", fixture.config.public_url.as_str())
        .output()
        .unwrap()
}

fn logged(output: Output) -> String {
    assert!(output.status.success(), "{output:?}");
    String::from_utf8(output.stderr).unwrap()
}

#[tokio::test]
async fn mails_the_reminders_due_once_and_logs_only_how_many() {
    let fixture = Fixture::start().await;
    let db = fixture.db().await;
    for account in ["alice", "bob"] {
        let member = NewMember {
            account: &MemberName::new(account),
            email: &format!("{account}@example.org"),
            news_opt_in: false,
            invited_by: None,
            joined_at: 1_760_000_000,
            is_admin: false,
        };
        db.insert_member(&member).await.unwrap();
    }
    let mailed_at = db::now() - REMINDER_AFTER;
    db.issue_verification_code("alice", mailed_at)
        .await
        .unwrap();

    let first = logged(remind(&fixture));
    let again = logged(remind(&fixture));

    assert_eq!(first, "Queued 1 reminder\n");
    assert_eq!(again, "Queued 0 reminders\n");
    let due = fixture.outbox().await.due(db::now()).await.unwrap();
    let mailed: Vec<_> = due
        .iter()
        .map(|message| (message.recipient.as_str(), message.subject.as_str()))
        .collect();
    assert_eq!(mailed, [("alice@example.org", REMINDER_SUBJECT)]);
    let alice = db.member("alice").await.unwrap().unwrap();
    assert!(alice.verification_reminded_at.is_some());
}

#[tokio::test]
async fn reminds_nobody_while_the_mailer_is_down_and_says_so() {
    let fixture = Fixture::start().await;
    let db = fixture.db().await;
    fixture.insert_member("alice", None).await;
    let mailed_at = db::now() - REMINDER_AFTER;
    db.issue_verification_code("alice", mailed_at)
        .await
        .unwrap();
    fixture.outbox().await.withdraw_key().await.unwrap();

    let output = remind(&fixture);

    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert_eq!(
        stderr,
        "Error: the mailer has published no key to seal mail to\n"
    );
    let alice = db.member("alice").await.unwrap().unwrap();
    assert_eq!(alice.verification_reminded_at, None);
}

#[test]
fn needs_the_public_url_for_its_links() {
    let output = Command::new(env!("CARGO_BIN_EXE_concierge"))
        .arg("remind")
        .env_clear()
        .env("STATE_DIRECTORY", "/nonexistent")
        .env("CONCIERGE_OUTBOX", "/nonexistent/outbox.db")
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(
        stderr.contains("CONCIERGE_PUBLIC_URL is not set"),
        "{stderr}"
    );
}
