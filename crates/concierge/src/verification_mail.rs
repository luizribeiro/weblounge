//! The emails that carry a member's verification link. Their subjects are
//! the same for everyone, so nothing a member chose reaches a header.

use askama::Template;

use crate::{
    db::{Db, IssuedCode},
    email::Email,
    invite_rules,
    outbox::{Message, Outbox, Subject},
    tree, verification_rules,
};

pub const SUBJECT: &str = "Confirm your email for weblounge.org";
/// Reminders can go out past the deadline, so the subject names no day.
pub const REMINDER_SUBJECT: &str = "Reminder: confirm your email for weblounge.org";

/// Which email carries the link.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// The first link, or one the member asked for.
    Link,
    /// The one reminder, for a member who hasn't confirmed.
    Reminder,
}

impl Kind {
    fn subject(self) -> &'static str {
        match self {
            Self::Link => SUBJECT,
            Self::Reminder => REMINDER_SUBJECT,
        }
    }
}

#[derive(Template)]
#[template(path = "verification-email.txt")]
struct Body<'a> {
    nick: &'a str,
    link: String,
    lifetime: String,
    grace_period: String,
    due: String,
    overdue: bool,
    reminder: bool,
}

/// The email of `kind` that carries `issued`'s link, written at `now`.
pub fn message(
    issued: &IssuedCode,
    public_url: &str,
    kind: Kind,
    now: i64,
) -> anyhow::Result<Message> {
    let body = Body {
        nick: &issued.account,
        link: format!("{public_url}/verify#{}", issued.code),
        lifetime: invite_rules::in_words(verification_rules::LIFETIME),
        grace_period: invite_rules::in_words(verification_rules::GRACE_PERIOD),
        due: tree::date(issued.deadline),
        overdue: now >= issued.deadline,
        reminder: kind == Kind::Reminder,
    };
    Ok(Message {
        account: Some(issued.account.clone()),
        to: Email::parse(&issued.email)?,
        subject: Subject::parse(kind.subject())?,
        body: body.render()?,
    })
}

/// Issues `account` a new link, replacing their last, and queues the
/// email of `kind` that carries it; answers whether they had an address
/// to verify. Issuing starts the grace period, so the email goes out
/// right after.
pub async fn send(
    db: &Db,
    outbox: &Outbox,
    public_url: &str,
    account: &str,
    kind: Kind,
    now: i64,
) -> anyhow::Result<bool> {
    let Some(issued) = db.issue_verification_code(account, now).await? else {
        return Ok(false);
    };
    outbox
        .queue(&message(&issued, public_url, kind, now)?, now)
        .await?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{db, nick, outbox};

    const PUBLIC_URL: &str = "https://members.weblounge.org";
    const SENT: i64 = 1_760_000_000;

    fn issued(account: &str) -> IssuedCode {
        IssuedCode {
            code: "q".repeat(verification_rules::CODE_LENGTH),
            account: account.into(),
            email: "bob@example.org".into(),
            expires_at: verification_rules::expires_at(SENT),
            deadline: verification_rules::deadline(SENT),
        }
    }

    fn link() -> String {
        format!(
            "{PUBLIC_URL}/verify#{}",
            "q".repeat(verification_rules::CODE_LENGTH)
        )
    }

    #[test]
    fn greets_the_member_by_nick_under_a_subject_that_never_names_them() {
        let message = message(&issued("Bob"), PUBLIC_URL, Kind::Link, SENT).unwrap();
        assert_eq!(message.subject.as_str(), SUBJECT);
        assert_eq!(message.to.as_str(), "bob@example.org");
        assert_eq!(message.account.as_deref(), Some("Bob"));
        assert!(message.body.starts_with("Hi Bob,\n"), "{}", message.body);
    }

    #[test]
    fn carries_the_link_on_a_line_of_its_own_and_says_how_long_it_has() {
        let body = message(&issued("Bob"), PUBLIC_URL, Kind::Link, SENT)
            .unwrap()
            .body;
        let links: Vec<_> = body.lines().filter(|line| line.contains("://")).collect();
        assert_eq!(links, [link()], "{body}");
        for part in [
            "The link works for 2 days.",
            "send\nyourself a new one from the Account tab.",
            "Every member has 7 days to confirm theirs; yours ends on 2025-10-16.",
            "You can chat right away; inviting others will need a confirmed email.",
            "If you didn't sign up for weblounge.org, you can ignore this email;",
        ] {
            assert!(body.contains(part), "{part}: {body}");
        }
    }

    #[test]
    fn says_when_the_grace_period_ended_once_it_has() {
        let deadline = verification_rules::deadline(SENT);
        let body = |now| {
            message(&issued("Bob"), PUBLIC_URL, Kind::Link, now)
                .unwrap()
                .body
        };
        assert!(body(deadline - 1).contains("yours ends on 2025-10-16."));
        assert!(body(deadline).contains("yours ended on 2025-10-16."));
    }

    #[test]
    fn a_reminder_says_the_old_links_stopped_working_and_why_to_confirm() {
        let late = verification_rules::deadline(SENT);
        let reminder = message(&issued("Bob"), PUBLIC_URL, Kind::Reminder, late).unwrap();
        assert_eq!(reminder.subject.as_str(), REMINDER_SUBJECT);
        let body = reminder.body;
        assert!(body.starts_with("Hi Bob,\n"), "{body}");
        let links: Vec<_> = body.lines().filter(|line| line.contains("://")).collect();
        assert_eq!(links, [link()], "{body}");
        for part in [
            "You haven't confirmed your email for weblounge.org yet, so here's a\n\
             new link; the ones we sent before don't work any more.",
            "The link works for 2 days.",
            "Your chat works as before, but inviting others needs a confirmed email.\n\
             Every member has 7 days to confirm theirs; yours ended on 2025-10-16.",
        ] {
            assert!(body.contains(part), "{part}: {body}");
        }
        assert!(!body.contains("right away"), "{body}");
    }

    /// Mail goes out as 7bit when it's ASCII in lines under 76 octets,
    /// CRLF included; the link alone can't fit.
    #[test]
    fn is_ascii_in_short_lines_but_for_the_link_even_for_the_longest_nick() {
        let longest = "B".repeat(nick::MAX_LENGTH);
        let late = verification_rules::deadline(SENT);
        for (kind, now) in [Kind::Link, Kind::Reminder]
            .into_iter()
            .flat_map(|kind| [(kind, SENT), (kind, late)])
        {
            let body = message(&issued(&longest), PUBLIC_URL, kind, now)
                .unwrap()
                .body;
            assert!(body.is_ascii(), "{body}");
            assert!(body.ends_with("your address.\n"), "{body:?}");
            for line in body.lines().filter(|line| *line != link()) {
                assert!(line.len() <= 73, "{line:?}");
            }
        }
    }

    #[tokio::test]
    async fn sending_queues_the_link_to_a_code_on_file() {
        let (db, _db_dir) = db::tests::temp_db().await;
        let (outbox, _outbox_dir) = outbox::tests::temp_outbox().await;
        db::tests::execute(
            &db,
            "INSERT INTO members (account, email, joined_at) VALUES ('Bob', 'bob@example.org', 0)",
        )
        .await
        .unwrap();

        assert!(
            send(&db, &outbox, PUBLIC_URL, "bob", Kind::Link, SENT)
                .await
                .unwrap()
        );

        let [queued] = &outbox.due(SENT).await.unwrap()[..] else {
            panic!("one message");
        };
        assert_eq!(queued.recipient, "bob@example.org");
        assert_eq!(queued.subject, SUBJECT);
        let prefix = format!("{PUBLIC_URL}/verify#");
        let code = queued
            .body
            .lines()
            .find_map(|line| line.strip_prefix(&prefix))
            .unwrap();
        let on_file = db.verification_code(code).await.unwrap().unwrap();
        assert_eq!(on_file.status(SENT), verification_rules::Status::Live);
        let bob = db.member("bob").await.unwrap().unwrap();
        assert_eq!(bob.verification_first_sent_at, Some(SENT));
    }

    #[tokio::test]
    async fn a_verified_member_gets_nothing() {
        let (db, _db_dir) = db::tests::temp_db().await;
        let (outbox, _outbox_dir) = outbox::tests::temp_outbox().await;
        db::tests::execute(
            &db,
            "INSERT INTO members (account, email, email_verified_at, joined_at)
             VALUES ('Bob', 'bob@example.org', 1, 0)",
        )
        .await
        .unwrap();

        assert!(
            !send(&db, &outbox, PUBLIC_URL, "bob", Kind::Link, SENT)
                .await
                .unwrap()
        );
        assert!(outbox.due(SENT).await.unwrap().is_empty());
    }
}
