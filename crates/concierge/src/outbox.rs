//! What concierge leaves in the outbox for concierge-mailer, and when the
//! mailer tries a message again after failing to send it. The outbox is a
//! database of its own, so the mailer opens nothing else of concierge's.
//!
//! Times are Unix seconds.

use std::{fmt, path::Path};

use anyhow::Context;
use sqlx::{SqlitePool, migrate::Migrator};

use crate::{email::Email, sqlite};

static MIGRATOR: Migrator = sqlx::migrate!("./outbox-migrations");

const MINUTE: i64 = 60;
const HOUR: i64 = 60 * MINUTE;
const DAY: i64 = 24 * HOUR;

/// The wait after the first failed attempt, doubling with each failure
/// after it.
pub const FIRST_RETRY: i64 = MINUTE;
/// The longest wait between attempts.
pub const MAX_RETRY: i64 = HOUR;
/// How long a message waits to be sent before the mailer gives up on it.
/// Verification links last this long, so a later message would carry a
/// dead link; and an address that can't be reached in two days is not
/// worth keeping on the disk.
pub const GIVE_UP_AFTER: i64 = 2 * DAY;

/// When to try a message again after its `attempts`th failure at `now`.
pub fn next_attempt_at(attempts: i64, now: i64) -> i64 {
    let doublings = attempts.saturating_sub(1).clamp(0, u32::MAX.into()) as u32;
    let wait = 2_i64.saturating_pow(doublings).saturating_mul(FIRST_RETRY);
    now + wait.min(MAX_RETRY)
}

/// The mailer has given up by `now` on the messages queued before this.
pub fn given_up_before(now: i64) -> i64 {
    now - GIVE_UP_AFTER
}

/// A subject line, which can't break out of its header.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Subject(String);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InvalidSubject;

impl fmt::Display for InvalidSubject {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("A subject is one line without control characters.")
    }
}

impl std::error::Error for InvalidSubject {}

impl Subject {
    pub fn parse(subject: &str) -> Result<Self, InvalidSubject> {
        if subject.chars().any(char::is_control) {
            return Err(InvalidSubject);
        }
        Ok(Self(subject.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A plain-text message to queue.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    /// The member it's for, if any, whose removal deletes it.
    pub account: Option<String>,
    pub to: Email,
    pub subject: Subject,
    pub body: String,
}

/// A message waiting in the outbox.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueuedMessage {
    pub id: i64,
    pub recipient: String,
    pub subject: String,
    pub body: String,
    pub created_at: i64,
    /// How many times sending it failed.
    pub attempts: i64,
}

/// What deleting messages from the outbox did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Deletion {
    pub messages: u64,
    /// Whether the deleted messages left the disk now, rather than at the
    /// next vacuum.
    pub off_the_disk: bool,
}

/// The outbox's SQLite database, which concierge writes and the mailer
/// empties.
#[derive(Clone)]
pub struct Outbox {
    pool: SqlitePool,
}

impl Outbox {
    /// Opens the outbox at `path`, which [`Outbox::migrate`] must have
    /// brought up to this build's schema.
    pub async fn open(path: &Path) -> anyhow::Result<Self> {
        let pool = sqlite::connect(path, false).await?;
        sqlite::ensure_migrated(&pool, &MIGRATOR, path).await?;
        Ok(Self { pool })
    }

    /// Opens the outbox at `path`, creating it if needed, and applies any
    /// migrations it lacks; only `concierge migrate` calls this, as for
    /// concierge's own database.
    pub async fn migrate(path: &Path) -> anyhow::Result<Self> {
        let pool = sqlite::connect(path, true).await?;
        MIGRATOR
            .run(&pool)
            .await
            .with_context(|| format!("can't migrate the outbox {}", path.display()))?;
        Ok(Self { pool })
    }

    /// The newest migration applied, if any.
    pub async fn migration_version(&self) -> sqlx::Result<Option<i64>> {
        sqlite::migration_version(&self.pool).await
    }

    /// Leaves `message` for the mailer to send as soon as it can, and
    /// answers its id, which the mailer logs it by.
    pub async fn queue(&self, message: &Message, now: i64) -> sqlx::Result<i64> {
        let (recipient, subject) = (message.to.as_str(), message.subject.as_str());
        sqlx::query!(
            "INSERT INTO outbox (account, recipient, subject, body, created_at, next_attempt_at)
             VALUES (?, ?, ?, ?, ?, ?)",
            message.account,
            recipient,
            subject,
            message.body,
            now,
            now,
        )
        .execute(&self.pool)
        .await
        .map(|inserted| inserted.last_insert_rowid())
    }

    /// The messages due to be tried at `now` that the mailer hasn't given
    /// up on, oldest first.
    pub async fn due(&self, now: i64) -> sqlx::Result<Vec<QueuedMessage>> {
        let given_up_before = given_up_before(now);
        sqlx::query_as!(
            QueuedMessage,
            "SELECT id, recipient, subject, body, created_at, attempts FROM outbox
             WHERE next_attempt_at <= ? AND created_at >= ?
             ORDER BY created_at, id",
            now,
            given_up_before,
        )
        .fetch_all(&self.pool)
        .await
    }

    /// Notes that sending `message` failed at `now`, putting it off.
    pub async fn record_failed_attempt(
        &self,
        message: &QueuedMessage,
        now: i64,
    ) -> sqlx::Result<()> {
        let next_attempt_at = next_attempt_at(message.attempts + 1, now);
        sqlx::query!(
            "UPDATE outbox SET attempts = attempts + 1, next_attempt_at = ? WHERE id = ?",
            next_attempt_at,
            message.id,
        )
        .execute(&self.pool)
        .await
        .map(drop)
    }

    /// Deletes a message once it's sent, or can never be.
    pub async fn delete(&self, id: i64) -> sqlx::Result<Deletion> {
        let deleted = sqlx::query!("DELETE FROM outbox WHERE id = ?", id)
            .execute(&self.pool)
            .await?;
        Ok(self.erase(deleted.rows_affected()).await)
    }

    /// Deletes the messages the mailer has given up on by `now`.
    pub async fn delete_given_up(&self, now: i64) -> sqlx::Result<Deletion> {
        let given_up_before = given_up_before(now);
        let deleted = sqlx::query!("DELETE FROM outbox WHERE created_at < ?", given_up_before)
            .execute(&self.pool)
            .await?;
        Ok(self.erase(deleted.rows_affected()).await)
    }

    /// Deletes the mail waiting for `account`, whose member was removed.
    pub async fn delete_for(&self, account: &str) -> sqlx::Result<Deletion> {
        let deleted = sqlx::query!("DELETE FROM outbox WHERE account = ?", account)
            .execute(&self.pool)
            .await?;
        Ok(self.erase(deleted.rows_affected()).await)
    }

    /// Takes the `messages` just deleted off the disk too.
    async fn erase(&self, messages: u64) -> Deletion {
        Deletion {
            messages,
            off_the_disk: messages == 0 || sqlite::empty_write_ahead_log(&self.pool).await,
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::sqlite::tests::files_contain;

    const NOW: i64 = 1_760_000_000;

    #[test]
    fn waits_double_from_a_minute_up_to_an_hour() {
        let waits: Vec<_> = (1..=9)
            .map(|attempts| next_attempt_at(attempts, NOW) - NOW)
            .collect();
        assert_eq!(
            waits,
            [1, 2, 4, 8, 16, 32, 60, 60, 60].map(|minutes| minutes * MINUTE)
        );
        assert_eq!(next_attempt_at(i64::MAX, NOW), NOW + MAX_RETRY);
    }

    #[test]
    fn gives_up_two_days_after_queueing() {
        assert_eq!(given_up_before(NOW + 2 * DAY), NOW);
    }

    #[test]
    fn a_subject_is_one_line() {
        let subject = "Verify your email for weblounge";
        assert_eq!(Subject::parse(subject).unwrap().as_str(), subject);
        for subject in [
            "Verify\r\nBcc: mallory@example.org",
            "Verify\nyour email",
            "Verify\ryour email",
            "Verify\0",
        ] {
            assert_eq!(Subject::parse(subject), Err(InvalidSubject), "{subject:?}");
        }
    }

    pub(crate) const QUEUED: i64 = 1_760_000_000;
    pub(crate) const VERIFY_SUBJECT: &str = "Verify your email for weblounge";

    pub(crate) async fn temp_outbox() -> (Outbox, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let outbox = Outbox::migrate(&dir.path().join("outbox.db"))
            .await
            .unwrap();
        (outbox, dir)
    }

    pub(crate) async fn execute(outbox: &Outbox, sql: &str) -> sqlx::Result<()> {
        sqlx::query(sql).execute(&outbox.pool).await.map(drop)
    }

    fn message(account: Option<&str>, to: &str, subject: &str) -> Message {
        Message {
            account: account.map(Into::into),
            to: Email::parse(to).unwrap(),
            subject: Subject::parse(subject).unwrap(),
            body: format!(
                "Hi {},\n\n\
                 Open https://members.weblounge.org/verify#q7Hx2mZp9vLcT4wRfBa to verify\n\
                 the address you gave weblounge.org. Zoë says hello.\n",
                account.unwrap_or("there"),
            ),
        }
    }

    /// alice's verification email, then bob's a minute later.
    pub(crate) async fn with_outbox() -> (Outbox, tempfile::TempDir) {
        let (outbox, dir) = temp_outbox().await;
        for (account, queued_at) in [("alice", QUEUED), ("bob", QUEUED + 60)] {
            let to = format!("{account}@example.org");
            let message = message(Some(account), &to, VERIFY_SUBJECT);
            outbox.queue(&message, queued_at).await.unwrap();
        }
        (outbox, dir)
    }

    fn recipients(messages: &[QueuedMessage]) -> Vec<&str> {
        messages
            .iter()
            .map(|message| message.recipient.as_str())
            .collect()
    }

    #[tokio::test]
    async fn opening_needs_an_outbox_migrated_by_this_build() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("outbox.db");
        assert!(Outbox::open(&path).await.is_err());
        assert!(!path.exists());

        Outbox::migrate(&path).await.unwrap();
        Outbox::migrate(&path).await.unwrap();

        let outbox = Outbox::open(&path).await.unwrap();
        let latest = sqlite::latest_migration(&MIGRATOR);
        assert_eq!(outbox.migration_version().await.unwrap(), Some(latest));
    }

    #[tokio::test]
    async fn queued_messages_are_due_at_once_oldest_first() {
        let (outbox, _dir) = with_outbox().await;
        let due = outbox.due(QUEUED + 60).await.unwrap();
        assert_eq!(recipients(&due), ["alice@example.org", "bob@example.org"]);
        assert_eq!(
            due[0],
            QueuedMessage {
                id: due[0].id,
                recipient: "alice@example.org".into(),
                subject: "Verify your email for weblounge".into(),
                body: message(Some("alice"), "alice@example.org", "-").body,
                created_at: QUEUED,
                attempts: 0,
            }
        );
        assert_eq!(
            recipients(&outbox.due(QUEUED).await.unwrap()),
            ["alice@example.org"]
        );
    }

    #[tokio::test]
    async fn nothing_is_due_before_anything_is_queued() {
        let (outbox, _dir) = temp_outbox().await;
        assert!(outbox.due(QUEUED).await.unwrap().is_empty());
        let (outbox, _dir) = with_outbox().await;
        assert!(outbox.due(QUEUED - 1).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn a_failed_attempt_puts_the_message_off() {
        let (outbox, _dir) = with_outbox().await;
        let alice = outbox.due(QUEUED).await.unwrap().remove(0);

        outbox.record_failed_attempt(&alice, QUEUED).await.unwrap();

        let retry_at = next_attempt_at(1, QUEUED);
        assert!(outbox.due(retry_at - 1).await.unwrap().is_empty());
        let due = outbox.due(retry_at).await.unwrap();
        assert_eq!(recipients(&due), ["alice@example.org", "bob@example.org"]);
        assert_eq!(due[0].attempts, 1);
        outbox
            .record_failed_attempt(&due[0], retry_at)
            .await
            .unwrap();
        let again_at = next_attempt_at(2, retry_at);
        let due = outbox.due(again_at - 1).await.unwrap();
        assert_eq!(recipients(&due), ["bob@example.org"]);
        let due = outbox.due(again_at).await.unwrap();
        assert_eq!(
            (due[0].recipient.as_str(), due[0].attempts),
            ("alice@example.org", 2)
        );
    }

    #[tokio::test]
    async fn messages_given_up_on_are_never_due_and_get_deleted() {
        let (outbox, _dir) = with_outbox().await;
        let alices_end = QUEUED + GIVE_UP_AFTER + 1;
        assert_eq!(
            recipients(&outbox.due(alices_end).await.unwrap()),
            ["bob@example.org"]
        );

        let deletion = outbox.delete_given_up(alices_end).await.unwrap();

        assert_eq!(
            deletion,
            Deletion {
                messages: 1,
                off_the_disk: true,
            }
        );
        let due = outbox.due(QUEUED + 60).await.unwrap();
        assert_eq!(recipients(&due), ["bob@example.org"]);
        let deletion = outbox.delete_given_up(alices_end).await.unwrap();
        assert_eq!(deletion.messages, 0);
    }

    #[tokio::test]
    async fn a_sent_message_leaves_the_database_files() {
        let (outbox, dir) = with_outbox().await;
        let secure_delete: i64 = sqlx::query_scalar("PRAGMA secure_delete")
            .fetch_one(&outbox.pool)
            .await
            .unwrap();
        assert_eq!(secure_delete, 1);
        let alice = outbox.due(QUEUED).await.unwrap().remove(0);

        let deletion = outbox.delete(alice.id).await.unwrap();

        assert_eq!(
            deletion,
            Deletion {
                messages: 1,
                off_the_disk: true,
            }
        );
        assert!(!files_contain(&dir, b"alice@example.org"));
        assert!(files_contain(&dir, b"bob@example.org"));
        assert_eq!(outbox.delete(alice.id).await.unwrap().messages, 0);
    }

    #[tokio::test]
    async fn a_removed_members_mail_leaves_but_not_others_mail_to_the_same_address() {
        let (outbox, dir) = with_outbox().await;
        for account in [Some("carol"), None] {
            let message = message(account, "alice@example.org", VERIFY_SUBJECT);
            outbox.queue(&message, QUEUED + 120).await.unwrap();
        }

        let deletion = outbox.delete_for("alice").await.unwrap();

        assert_eq!(
            deletion,
            Deletion {
                messages: 1,
                off_the_disk: true,
            }
        );
        assert!(!files_contain(&dir, b"Hi alice,"));
        let due = outbox.due(QUEUED + 120).await.unwrap();
        assert_eq!(
            recipients(&due),
            ["bob@example.org", "alice@example.org", "alice@example.org"]
        );
        assert_eq!(outbox.delete_for("alice").await.unwrap().messages, 0);
    }
}
