//! The emails that carry a member's verification link. Their subjects are
//! the same for everyone, so nothing a member chose reaches a header, and
//! they're queued sealed to the mailer's key, so the link never reaches
//! the disk.

use askama::Template;
use outbox::{Message, Outbox, Subject};
use secret::SecretString;

use crate::{
    db::{Db, IssuedCode},
    invite_rules, link_mail,
    name::MemberName,
    rate_limit, verification_rules,
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

/// The most links one run of a batch mails: the site's own hourly share
/// again, so a batch run hourly leaves the mailer room for the links
/// members ask for.
pub const PER_RUN: usize = rate_limit::LINKS_PER_HOUR;

/// What mailing a batch of links did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Batch {
    /// The accounts that were mailed a link.
    pub sent: Vec<MemberName>,
    /// How many accounts are left for another run.
    pub left: usize,
}

#[derive(Template)]
#[template(path = "verification-email.txt")]
struct Body<'a> {
    nick: &'a str,
    link: SecretString,
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
        nick: issued.account.as_str(),
        link: format!("{public_url}/verify#{}", issued.code.expose()).into(),
        lifetime: invite_rules::in_words(verification_rules::LIFETIME),
        grace_period: invite_rules::in_words(verification_rules::GRACE_PERIOD),
        due: crate::tree::day_in_words(issued.deadline),
        overdue: now >= issued.deadline,
        reminder: kind == Kind::Reminder,
    };
    Ok(Message {
        account: Some(issued.account.to_string()),
        to: issued.email.clone(),
        subject: Subject::parse(kind.subject())?,
        body: body.render()?,
    })
}

/// Issues `account` a new link, replacing their last, and queues the
/// email of `kind` that carries it, sealed; answers its id, unless they
/// had no address to verify. Issuing starts the grace period, so the email goes
/// out right after. Fails with [`outbox::NoKey`] while the mailer is down.
pub async fn send(
    db: &Db,
    outbox: &Outbox,
    public_url: &str,
    account: &MemberName,
    kind: Kind,
    now: i64,
) -> anyhow::Result<Option<i64>> {
    let issue = db.issue_verification_code(account, now);
    let write = |issued: &IssuedCode| message(issued, public_url, kind, now);
    let Some((id, issued)) = link_mail::send(outbox, now, issue, write).await? else {
        return Ok(None);
    };
    // Noted once queued: a failure in between leaves a reminder that may
    // go out twice, two days apart, rather than one that never does.
    if kind == Kind::Reminder {
        db.record_reminder(&issued.account, now).await?;
    }
    Ok(Some(id))
}

/// [`send`]s a link of `kind` to each of `accounts` in turn, up to
/// [`PER_RUN`] of them.
pub async fn send_batch(
    db: &Db,
    outbox: &Outbox,
    public_url: &str,
    accounts: &[MemberName],
    kind: Kind,
    now: i64,
) -> anyhow::Result<Batch> {
    let mut sent = Vec::new();
    for account in accounts.iter().take(PER_RUN) {
        if send(db, outbox, public_url, account, kind, now)
            .await?
            .is_some()
        {
            sent.push(account.clone());
        }
    }
    Ok(Batch {
        sent,
        left: accounts.len().saturating_sub(PER_RUN),
    })
}

/// Mails a reminder to every member it's due at `now`, as many as one
/// batch takes. A link a member asks for while the batch runs can be
/// replaced by their reminder, which carries a working link of its own.
pub async fn remind(db: &Db, outbox: &Outbox, public_url: &str, now: i64) -> anyhow::Result<Batch> {
    let due: Vec<_> = db
        .verification_progress()
        .await?
        .into_iter()
        .filter(|(_, progress)| progress.reminder_due(now))
        .map(|(account, _)| account)
        .collect();
    send_batch(db, outbox, public_url, &due, Kind::Reminder, now).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{db, link_code, nick};
    use outbox::{Body, NoKey, testing::TestKey};

    const PUBLIC_URL: &str = "https://members.weblounge.org";
    const SENT: i64 = 1_760_000_000;

    fn issued(account: &str) -> IssuedCode {
        IssuedCode {
            code: "q".repeat(link_code::MAILED_LENGTH).into(),
            account: MemberName::new(account),
            email: db::tests::email("bob@example.org"),
            expires_at: verification_rules::expires_at(SENT),
            deadline: verification_rules::deadline(SENT),
        }
    }

    fn bob() -> MemberName {
        MemberName::new("bob")
    }

    /// The names a batch mailed, as spelled.
    fn sent(batch: &Batch) -> Vec<&str> {
        batch.sent.iter().map(MemberName::as_str).collect()
    }

    fn link() -> String {
        format!(
            "{PUBLIC_URL}/verify#{}",
            "q".repeat(link_code::MAILED_LENGTH)
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
            "Every member has 7 days to confirm theirs;\nyours ends on 16 October.",
            "You can chat right away, but you won't be able to invite others until\n\
             you confirm your email.",
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
        assert!(body(deadline - 1).contains("yours ends on 16 October."));
        assert!(body(deadline).contains("yours ended on 16 October."));
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
            "You can chat right away, but you won't be able to invite others until\n\
             you confirm your email. Every member has 7 days to confirm theirs;\n\
             yours ended on 16 October.",
        ] {
            assert!(body.contains(part), "{part}: {body}");
        }
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

    /// A database with Bob in it, and an outbox the mailer published
    /// `key` in.
    async fn with_bob() -> (Db, Outbox, TestKey, [tempfile::TempDir; 2]) {
        let (db, db_dir) = db::tests::temp_db().await;
        let (outbox, outbox_dir) = outbox::testing::temp_outbox().await;
        let key = TestKey::publish(&outbox).await;
        db::tests::execute(
            &db,
            "INSERT INTO members (account, email, joined_at) VALUES ('Bob', 'bob@example.org', 0)",
        )
        .await
        .unwrap();
        (db, outbox, key, [db_dir, outbox_dir])
    }

    #[tokio::test]
    async fn sending_queues_the_link_to_a_code_on_file_sealed() {
        let (db, outbox, key, _dirs) = with_bob().await;

        let sent = send(&db, &outbox, PUBLIC_URL, &bob(), Kind::Link, SENT).await;

        let due = outbox.due(SENT).await.unwrap();
        assert_eq!(sent.unwrap(), Some(due[0].id));
        assert!(matches!(due[0].body, Body::Sealed { .. }), "{due:?}");
        let [queued] = &key.open(due)[..] else {
            panic!("one message");
        };
        assert_eq!(queued.recipient, "bob@example.org");
        assert_eq!(queued.subject, SUBJECT);
        let lapsed = verification_rules::expires_at(SENT);
        assert_eq!(outbox.due(lapsed - 1).await.unwrap().len(), 1);
        assert!(outbox.due(lapsed).await.unwrap().is_empty());
        let prefix = format!("{PUBLIC_URL}/verify#");
        let code = queued
            .text()
            .lines()
            .find_map(|line| line.strip_prefix(&prefix))
            .unwrap();
        let on_file = db.verification_code(code).await.unwrap().unwrap();
        assert_eq!(on_file.status(SENT), verification_rules::Status::Live);
        let bob = db.member("bob").await.unwrap().unwrap();
        assert_eq!(bob.verification_first_sent_at, Some(SENT));
    }

    #[tokio::test]
    async fn without_the_mailers_key_nothing_is_issued_or_queued() {
        let (db, outbox, _key, _dirs) = with_bob().await;
        outbox.withdraw_key().await.unwrap();

        let error = send(&db, &outbox, PUBLIC_URL, &bob(), Kind::Link, SENT)
            .await
            .unwrap_err();

        assert_eq!(error.downcast_ref(), Some(&NoKey));
        assert!(outbox.due(SENT).await.unwrap().is_empty());
        let bob = db.member("bob").await.unwrap().unwrap();
        assert_eq!(bob.verification_first_sent_at, None);
    }

    #[tokio::test]
    async fn a_verified_member_gets_nothing() {
        let (db, outbox, _key, _dirs) = with_bob().await;
        db::tests::execute(&db, "UPDATE members SET email_verified_at = 1")
            .await
            .unwrap();

        let sent = send(&db, &outbox, PUBLIC_URL, &bob(), Kind::Link, SENT).await;
        assert_eq!(sent.unwrap(), None);
        assert!(outbox.due(SENT).await.unwrap().is_empty());
    }

    mod remind {
        use super::*;

        const DAY: i64 = 24 * 60 * 60;
        const NOW: i64 = SENT + verification_rules::REMINDER_AFTER;

        async fn with_members(accounts: &[&str]) -> (Db, Outbox, [tempfile::TempDir; 2]) {
            let (db, db_dir) = db::tests::temp_db().await;
            let (outbox, outbox_dir) = outbox::testing::temp_outbox().await;
            TestKey::publish(&outbox).await;
            for account in accounts {
                db::tests::execute(
                    &db,
                    &format!(
                        "INSERT INTO members (account, email, joined_at)
                         VALUES ('{account}', '{account}@example.org', 0)"
                    ),
                )
                .await
                .unwrap();
            }
            (db, outbox, [db_dir, outbox_dir])
        }

        async fn mail(db: &Db, account: &str, at: i64) {
            db.issue_verification_code(&MemberName::new(account), at)
                .await
                .unwrap()
                .unwrap();
        }

        /// What reminding at NOW queued.
        async fn queued(outbox: &Outbox) -> Vec<(String, String)> {
            let due = outbox.due(NOW).await.unwrap();
            due.into_iter()
                .map(|message| (message.recipient, message.subject))
                .collect()
        }

        #[tokio::test]
        async fn reminds_only_the_unverified_mailed_five_days_ago_and_not_reminded_since() {
            let (db, outbox, _dirs) =
                with_members(&["due", "early", "verified", "unmailed", "reminded", "resent"]).await;
            for account in ["due", "verified", "reminded", "resent"] {
                mail(&db, account, SENT).await;
            }
            mail(&db, "early", SENT + DAY).await;
            mail(&db, "resent", NOW - DAY).await;
            db.record_reminder(&MemberName::new("reminded"), SENT + DAY)
                .await
                .unwrap();
            db::tests::execute(
                &db,
                "UPDATE members SET email_verified_at = 1 WHERE account = 'verified'",
            )
            .await
            .unwrap();

            let batch = remind(&db, &outbox, PUBLIC_URL, NOW).await.unwrap();

            assert_eq!(
                batch,
                Batch {
                    sent: vec![MemberName::new("due")],
                    left: 0
                }
            );
            assert_eq!(
                queued(&outbox).await,
                [("due@example.org".into(), REMINDER_SUBJECT.into())]
            );
            let due = db.member("due").await.unwrap().unwrap();
            assert_eq!(due.verification_reminded_at, Some(NOW));
            assert_eq!(due.verification_first_sent_at, Some(SENT));
        }

        #[tokio::test]
        async fn running_again_sends_nothing_more() {
            let (db, outbox, _dirs) = with_members(&["due"]).await;
            mail(&db, "due", SENT).await;
            remind(&db, &outbox, PUBLIC_URL, NOW).await.unwrap();

            for later in [NOW + 60, NOW + 3 * DAY] {
                let batch = remind(&db, &outbox, PUBLIC_URL, later).await.unwrap();
                assert_eq!(
                    batch,
                    Batch {
                        sent: vec![],
                        left: 0
                    }
                );
            }
            assert_eq!(queued(&outbox).await.len(), 1);
        }

        #[tokio::test]
        async fn a_link_asked_for_on_day_four_puts_the_reminder_off_two_days() {
            let (db, outbox, _dirs) = with_members(&["resent"]).await;
            mail(&db, "resent", SENT).await;
            mail(&db, "resent", SENT + 4 * DAY).await;

            let on_day_five = remind(&db, &outbox, PUBLIC_URL, NOW).await.unwrap();
            let lapsed = verification_rules::expires_at(SENT + 4 * DAY);
            let once_lapsed = remind(&db, &outbox, PUBLIC_URL, lapsed).await.unwrap();

            assert!(on_day_five.sent.is_empty());
            assert_eq!(sent(&once_lapsed), ["resent"]);
        }

        #[tokio::test]
        async fn mails_one_batch_a_run_and_leaves_the_rest_for_the_next() {
            let accounts: Vec<_> = (0..PER_RUN + 2).map(|n| format!("m{n:03}")).collect();
            let names: Vec<_> = accounts.iter().map(String::as_str).collect();
            let (db, outbox, _dirs) = with_members(&names).await;
            for account in &names {
                mail(&db, account, SENT).await;
            }

            let first = remind(&db, &outbox, PUBLIC_URL, NOW).await.unwrap();
            let second = remind(&db, &outbox, PUBLIC_URL, NOW + 60 * 60)
                .await
                .unwrap();

            assert_eq!((first.sent.len(), first.left), (PER_RUN, 2));
            assert_eq!(sent(&second), names[PER_RUN..]);
            assert_eq!(second.left, 0);
        }
    }
}
