//! The emails of a password reset: the link, sealed like verification
//! links, and the notice once the password changed, which carries no
//! secret and goes as it is. Their subjects are the same for everyone.

use askama::Template;
use outbox::{Message, Outbox, Subject};

use crate::{
    db::{Db, IssuedReset},
    email::Email,
    invite_rules, link_mail,
    name::MemberName,
    reset_rules,
};

pub const LINK_SUBJECT: &str = "Reset your password for weblounge.org";
pub const CHANGED_SUBJECT: &str = "Your password for weblounge.org was changed";
/// Where members ask for a reset link.
pub const FORGOT_PATH: &str = "/forgot-password";
/// Where a reset link leads; the code follows in the fragment.
pub const RESET_PATH: &str = "/reset";

#[derive(Template)]
#[template(path = "reset-email.txt")]
struct LinkBody<'a> {
    nick: &'a str,
    link: String,
    lifetime: String,
}

#[derive(Template)]
#[template(path = "password-changed-email.txt")]
struct ChangedBody<'a> {
    nick: &'a str,
    forgot_link: String,
}

/// The email that carries `issued`'s link.
pub fn link_message(issued: &IssuedReset, public_url: &str) -> anyhow::Result<Message> {
    let body = LinkBody {
        nick: issued.member.account.as_str(),
        link: format!("{public_url}{RESET_PATH}#{}", issued.code),
        lifetime: invite_rules::in_words(reset_rules::LIFETIME),
    };
    message(
        &issued.member.account,
        &issued.member.email,
        LINK_SUBJECT,
        body.render()?,
    )
}

/// The email that tells `account`, at `email`, that a link reset their
/// password.
pub fn changed_message(
    account: &MemberName,
    email: &str,
    public_url: &str,
) -> anyhow::Result<Message> {
    let body = ChangedBody {
        nick: account.as_str(),
        forgot_link: format!("{public_url}{FORGOT_PATH}"),
    };
    message(account, email, CHANGED_SUBJECT, body.render()?)
}

fn message(
    account: &MemberName,
    email: &str,
    subject: &str,
    body: String,
) -> anyhow::Result<Message> {
    Ok(Message {
        account: Some(account.to_string()),
        to: Email::parse(email)?,
        subject: Subject::parse(subject)?,
        body,
    })
}

/// Issues `account` a reset link, replacing their last, and queues the
/// email that carries it, sealed; answers its id, unless they have no
/// confirmed email. Fails with [`outbox::NoKey`] while the mailer is down.
pub async fn send_link(
    db: &Db,
    outbox: &Outbox,
    public_url: &str,
    account: &MemberName,
    now: i64,
) -> anyhow::Result<Option<i64>> {
    let issue = db.issue_reset_code(account, now);
    let write = |issued: &IssuedReset| link_message(issued, public_url);
    let sent = link_mail::send(outbox, now, issue, write).await?;
    Ok(sent.map(|(id, _)| id))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{db, link_code, nick};
    use outbox::{Body, NoKey, testing::TestKey};

    const PUBLIC_URL: &str = "https://members.weblounge.org";
    const SENT: i64 = 1_760_000_000;

    fn issued(account: &str) -> IssuedReset {
        IssuedReset {
            code: "q".repeat(link_code::MAILED_LENGTH),
            member: db::Recoverable {
                account: MemberName::new(account),
                email: "bob@example.org".into(),
            },
            expires_at: reset_rules::expires_at(SENT),
        }
    }

    fn link() -> String {
        format!(
            "{PUBLIC_URL}/reset#{}",
            "q".repeat(link_code::MAILED_LENGTH)
        )
    }

    fn links(body: &str) -> Vec<&str> {
        body.lines().filter(|line| line.contains("://")).collect()
    }

    #[test]
    fn the_link_goes_to_the_confirmed_address_under_a_subject_that_never_names_anyone() {
        let message = link_message(&issued("Bob"), PUBLIC_URL).unwrap();
        assert_eq!(message.subject.as_str(), LINK_SUBJECT);
        assert_eq!(message.to.as_str(), "bob@example.org");
        assert_eq!(message.account.as_deref(), Some("Bob"));
        let body = message.body;
        assert!(body.starts_with("Hi Bob,\n"), "{body}");
        assert_eq!(links(&body), [link()], "{body}");
        for part in [
            "press\n\"Set my password\"",
            "The link works once, within 1 hour.",
            "If you didn't ask to reset\nyour password, you can ignore this email",
        ] {
            assert!(body.contains(part), "{part}: {body}");
        }
    }

    #[test]
    fn the_notice_says_what_changed_and_where_to_go_if_it_wasnt_them() {
        let bob = MemberName::new("Bob");
        let message = changed_message(&bob, "bob@example.org", PUBLIC_URL).unwrap();
        assert_eq!(message.subject.as_str(), CHANGED_SUBJECT);
        assert_eq!(message.to.as_str(), "bob@example.org");
        let body = message.body;
        assert!(body.starts_with("Hi Bob,\n"), "{body}");
        assert!(body.contains("was just changed"), "{body}");
        assert!(body.contains("signed out of the members' site"), "{body}");
        assert!(
            body.contains(&format!("{PUBLIC_URL}/forgot-password and tell an admin.")),
            "{body}"
        );
    }

    /// Mail goes out as 7bit when it's ASCII in lines under 76 octets,
    /// CRLF included; the link alone can't fit.
    #[test]
    fn both_are_ascii_in_short_lines_but_for_the_link_even_for_the_longest_nick() {
        let longest = "B".repeat(nick::MAX_LENGTH);
        for message in [
            link_message(&issued(&longest), PUBLIC_URL).unwrap(),
            changed_message(&MemberName::new(&longest), "bob@example.org", PUBLIC_URL).unwrap(),
        ] {
            let body = message.body;
            assert!(body.is_ascii(), "{body}");
            assert!(body.ends_with(".\n"), "{body:?}");
            for line in body.lines().filter(|line| *line != link()) {
                assert!(line.len() <= 73, "{line:?}");
            }
        }
    }

    async fn with_carol() -> (Db, Outbox, TestKey, [tempfile::TempDir; 2]) {
        let (db, db_dir) = db::tests::temp_db().await;
        let (outbox, outbox_dir) = outbox::testing::temp_outbox().await;
        let key = TestKey::publish(&outbox).await;
        db::tests::execute(
            &db,
            "INSERT INTO members (account, email, email_verified_at, joined_at)
             VALUES ('carol', 'carol@example.org', 1, 0), ('dave', 'dave@example.org', NULL, 0)",
        )
        .await
        .unwrap();
        (db, outbox, key, [db_dir, outbox_dir])
    }

    #[tokio::test]
    async fn sending_queues_the_link_sealed_until_it_stops_working() {
        let (db, outbox, key, _dirs) = with_carol().await;

        let carol = MemberName::new("Carol");
        let sent = send_link(&db, &outbox, PUBLIC_URL, &carol, SENT).await;

        let due = outbox.due(SENT).await.unwrap();
        assert_eq!(sent.unwrap(), Some(due[0].id));
        assert!(matches!(due[0].body, Body::Sealed { .. }), "{due:?}");
        let [queued] = &key.open(due)[..] else {
            panic!("one message");
        };
        assert_eq!(
            (queued.recipient.as_str(), queued.subject.as_str()),
            ("carol@example.org", LINK_SUBJECT)
        );
        let prefix = format!("{PUBLIC_URL}/reset#");
        let code = queued
            .text()
            .lines()
            .find_map(|line| line.strip_prefix(&prefix))
            .unwrap();
        let lapses = reset_rules::expires_at(SENT);
        assert!(outbox.due(lapses).await.unwrap().is_empty());
        let taken = db.take_reset_code(code, lapses - 1).await.unwrap();
        assert_eq!(taken.unwrap().account.as_str(), "carol");
    }

    #[tokio::test]
    async fn an_unconfirmed_member_gets_nothing() {
        let (db, outbox, _key, _dirs) = with_carol().await;
        let dave = MemberName::new("dave");
        let sent = send_link(&db, &outbox, PUBLIC_URL, &dave, SENT).await;
        assert_eq!(sent.unwrap(), None);
        assert!(outbox.due(SENT).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn without_the_mailers_key_no_code_is_issued_and_the_last_keeps_working() {
        let (db, outbox, _key, _dirs) = with_carol().await;
        let carol = MemberName::new("carol");
        let last = db.issue_reset_code(&carol, SENT).await.unwrap().unwrap();
        outbox.withdraw_key().await.unwrap();

        let error = send_link(&db, &outbox, PUBLIC_URL, &carol, SENT)
            .await
            .unwrap_err();

        assert_eq!(error.downcast_ref(), Some(&NoKey));
        assert!(outbox.due(SENT).await.unwrap().is_empty());
        assert!(
            db.take_reset_code(&last.code, SENT)
                .await
                .unwrap()
                .is_some()
        );
    }
}
