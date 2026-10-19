//! What concierge leaves in the outbox for concierge-mailer, and when the
//! mailer tries a message again after failing to send it. The outbox is a
//! database of its own, so the mailer opens nothing else of concierge's;
//! the mailer publishes its public key in it too. Concierge seals the
//! messages that carry a secret to that key, and only the mailer opens
//! them.
//!
//! Times are Unix seconds.

use std::{fmt, path::Path};

use anyhow::Context;
use common::{email::Email, sqlite};
use sqlx::{SqliteConnection, SqliteExecutor, SqlitePool, migrate::Migrator};

/// The public half of the mailer's key, which mail is sealed to.
pub use age::x25519::Recipient as PublicKey;

static MIGRATOR: Migrator = sqlx::migrate!();

const MINUTE: i64 = 60;
const HOUR: i64 = 60 * MINUTE;
const DAY: i64 = 24 * HOUR;

/// The wait after the first failed attempt, doubling with each failure
/// after it.
pub const FIRST_RETRY: i64 = MINUTE;
/// The longest wait between attempts.
pub const MAX_RETRY: i64 = HOUR;
/// How long a message waits to be sent before the mailer gives up on it,
/// unless it was queued to be given up on sooner: an address that can't
/// be reached in two days is not worth keeping on the disk.
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

// Only ever written: what's read back stays text, which the mailer parses
// again before sending.
common::sqlite_newtype!(Subject, 0: String);

/// A plain-text message to queue, as it is or sealed. Its `Debug` output
/// leaves the body out: a sealed message's carries a secret link.
#[derive(Clone, PartialEq, Eq)]
pub struct Message {
    /// The member it's for, if any, whose removal deletes it.
    pub account: Option<String>,
    pub to: Email,
    pub subject: Subject,
    pub body: String,
}

impl fmt::Debug for Message {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Message")
            .field("account", &self.account)
            .field("to", &self.to)
            .field("subject", &self.subject)
            .field("body", &format_args!("<redacted>"))
            .finish()
    }
}

/// What a queued message says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Body {
    Plain(String),
    /// age ciphertext, which only the mailer that published `to` opens.
    Sealed {
        to: String,
        ciphertext: Vec<u8>,
    },
}

/// A message waiting in the outbox.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueuedMessage {
    pub id: i64,
    pub recipient: String,
    pub subject: String,
    pub body: Body,
    pub created_at: i64,
    /// How many times sending it failed.
    pub attempts: i64,
}

/// An outbox row, whose CHECKs make it plain or sealed.
struct Row {
    id: i64,
    recipient: String,
    subject: String,
    body: Option<String>,
    sealed_to: Option<String>,
    sealed_body: Option<Vec<u8>>,
    created_at: i64,
    attempts: i64,
}

impl From<Row> for QueuedMessage {
    fn from(row: Row) -> Self {
        let body = match row.body {
            Some(text) => Body::Plain(text),
            None => Body::Sealed {
                to: row.sealed_to.unwrap_or_default(),
                ciphertext: row.sealed_body.unwrap_or_default(),
            },
        };
        Self {
            id: row.id,
            recipient: row.recipient,
            subject: row.subject,
            body,
            created_at: row.created_at,
            attempts: row.attempts,
        }
    }
}

/// Why a message wasn't sealed: the mailer isn't running, or hasn't
/// published its key yet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NoKey;

impl fmt::Display for NoKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("the mailer has published no key to seal mail to")
    }
}

impl std::error::Error for NoKey {}

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

    /// Makes `key` the one concierge seals mail to, in place of any other,
    /// and deletes what was sealed to another: whoever held that key is
    /// gone, and nobody can open it now. Sealing reads the key in the same
    /// transaction it queues in, so nothing sealed to the old key can be
    /// queued after this.
    pub async fn publish_key(&self, key: &PublicKey) -> sqlx::Result<Deletion> {
        let key = key.to_string();
        let mut transaction = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        sqlx::query!(
            "INSERT INTO mailer_key (only, public_key) VALUES (1, ?)
             ON CONFLICT (only) DO UPDATE SET public_key = excluded.public_key",
            key,
        )
        .execute(&mut *transaction)
        .await?;
        let deleted = delete_sealed_to_others(&mut transaction, Some(&key)).await?;
        transaction.commit().await?;
        Ok(self.erase(deleted).await)
    }

    /// Withdraws the published key as its mailer stops, and deletes what
    /// was sealed to it, which nobody can open once that mailer is gone.
    /// Until the next mailer publishes its key, nothing is sealed.
    pub async fn withdraw_key(&self) -> sqlx::Result<Deletion> {
        let mut transaction = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        sqlx::query!("DELETE FROM mailer_key")
            .execute(&mut *transaction)
            .await?;
        let deleted = delete_sealed_to_others(&mut transaction, None).await?;
        transaction.commit().await?;
        Ok(self.erase(deleted).await)
    }

    /// Deletes the messages sealed to any key but `key`, which nobody can
    /// open any more.
    pub async fn delete_sealed_to_others(&self, key: &PublicKey) -> sqlx::Result<Deletion> {
        let mut transaction = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let deleted = delete_sealed_to_others(&mut transaction, Some(&key.to_string())).await?;
        transaction.commit().await?;
        Ok(self.erase(deleted).await)
    }

    /// The ids of the sealed messages deleted unsent since this was last
    /// asked, forgetting them.
    pub async fn take_unsent(&self) -> sqlx::Result<Vec<i64>> {
        sqlx::query_scalar!("DELETE FROM unsent RETURNING id")
            .fetch_all(&self.pool)
            .await
    }

    /// The key the mailer published, if any.
    pub async fn published_key(&self) -> sqlx::Result<Option<PublicKey>> {
        published_key(&self.pool).await
    }

    /// Leaves `message` for the mailer to send as soon as it can, and
    /// answers its id, which the mailer logs it by.
    pub async fn queue(&self, message: &Message, now: i64) -> sqlx::Result<i64> {
        let body = Body::Plain(message.body.clone());
        insert(&self.pool, message, &body, now, None).await
    }

    /// [`queue`](Self::queue)s `message` sealed to the mailer's key, so the
    /// disk never holds what it says, for the mailer to give up on by
    /// `give_up_at` at the latest, when the secret it carries stops
    /// working. Fails with [`NoKey`] while the mailer has published none,
    /// rather than leave the message as it is.
    pub async fn queue_sealed(
        &self,
        message: &Message,
        now: i64,
        give_up_at: i64,
    ) -> anyhow::Result<i64> {
        let mut transaction = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let key = published_key(&mut *transaction).await?.ok_or(NoKey)?;
        let body = Body::Sealed {
            to: key.to_string(),
            ciphertext: age::encrypt(&key, message.body.as_bytes())?,
        };
        let id = insert(&mut *transaction, message, &body, now, Some(give_up_at)).await?;
        transaction.commit().await?;
        Ok(id)
    }

    /// The messages due to be tried at `now` that the mailer hasn't given
    /// up on, oldest first.
    pub async fn due(&self, now: i64) -> sqlx::Result<Vec<QueuedMessage>> {
        let given_up_before = given_up_before(now);
        let rows = sqlx::query_as!(
            Row,
            "SELECT id, recipient, subject, body, sealed_to, sealed_body, created_at, attempts
             FROM outbox
             WHERE next_attempt_at <= ? AND created_at >= ? AND (give_up_at IS NULL OR give_up_at > ?)
             ORDER BY created_at, id",
            now,
            given_up_before,
            now,
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(QueuedMessage::from).collect())
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

    /// Deletes a message once it's sent, or refused for good.
    pub async fn delete(&self, id: i64) -> sqlx::Result<Deletion> {
        let deleted = sqlx::query!("DELETE FROM outbox WHERE id = ?", id)
            .execute(&self.pool)
            .await?;
        Ok(self.erase(deleted.rows_affected()).await)
    }

    /// Deletes a message that can never be sent, noting it unsent if it
    /// was sealed.
    pub async fn delete_unsent(&self, id: i64) -> sqlx::Result<Deletion> {
        let mut transaction = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        sqlx::query!(
            "INSERT OR IGNORE INTO unsent (id)
             SELECT id FROM outbox WHERE id = ? AND sealed_to IS NOT NULL",
            id,
        )
        .execute(&mut *transaction)
        .await?;
        let deleted = sqlx::query!("DELETE FROM outbox WHERE id = ?", id)
            .execute(&mut *transaction)
            .await?;
        transaction.commit().await?;
        Ok(self.erase(deleted.rows_affected()).await)
    }

    /// Deletes the messages the mailer has given up on by `now`.
    pub async fn delete_given_up(&self, now: i64) -> sqlx::Result<Deletion> {
        let given_up_before = given_up_before(now);
        let deleted = sqlx::query!(
            "DELETE FROM outbox WHERE created_at < ? OR give_up_at <= ?",
            given_up_before,
            now,
        )
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

async fn insert(
    executor: impl SqliteExecutor<'_>,
    message: &Message,
    body: &Body,
    now: i64,
    give_up_at: Option<i64>,
) -> sqlx::Result<i64> {
    let (text, sealed_to, sealed_body) = match body {
        Body::Plain(text) => (Some(text), None, None),
        Body::Sealed { to, ciphertext } => (None, Some(to), Some(ciphertext)),
    };
    sqlx::query!(
        "INSERT INTO outbox
             (account, recipient, subject, body, sealed_to, sealed_body, created_at, next_attempt_at,
              give_up_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
        message.account,
        message.to,
        message.subject,
        text,
        sealed_to,
        sealed_body,
        now,
        now,
        give_up_at,
    )
    .execute(executor)
    .await
    .map(|inserted| inserted.last_insert_rowid())
}

/// Deletes what was sealed to any key but `key`, or to any key at all,
/// noting it unsent; answers how many messages that was.
async fn delete_sealed_to_others(
    transaction: &mut SqliteConnection,
    key: Option<&str>,
) -> sqlx::Result<u64> {
    sqlx::query!(
        "INSERT OR IGNORE INTO unsent (id)
         SELECT id FROM outbox WHERE sealed_to IS NOT NULL AND sealed_to IS NOT ?",
        key,
    )
    .execute(&mut *transaction)
    .await?;
    let deleted = sqlx::query!(
        "DELETE FROM outbox WHERE sealed_to IS NOT NULL AND sealed_to IS NOT ?",
        key,
    )
    .execute(&mut *transaction)
    .await?;
    Ok(deleted.rows_affected())
}

async fn published_key(executor: impl SqliteExecutor<'_>) -> sqlx::Result<Option<PublicKey>> {
    let key = sqlx::query_scalar!("SELECT public_key FROM mailer_key")
        .fetch_optional(executor)
        .await?;
    key.map(|key| {
        key.parse()
            .map_err(|error: &str| sqlx::Error::Decode(error.into()))
    })
    .transpose()
}

/// A migrated outbox, and the mail tests queue in it.
#[cfg(any(test, feature = "testing"))]
pub mod testing {
    use super::*;

    pub const QUEUED: i64 = 1_760_000_000;
    /// A time to give up on sealed mail that's no sooner than the outbox
    /// gives up on any.
    pub const NO_SOONER: i64 = i64::MAX;
    pub const VERIFY_SUBJECT: &str = "Verify your email for weblounge";

    pub async fn temp_outbox() -> (Outbox, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let outbox = Outbox::migrate(&dir.path().join("outbox.db"))
            .await
            .unwrap();
        (outbox, dir)
    }

    pub async fn execute(outbox: &Outbox, sql: &str) -> sqlx::Result<()> {
        sqlx::query(sql).execute(&outbox.pool).await.map(drop)
    }

    pub fn message(account: Option<&str>, to: &str, subject: &str) -> Message {
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

    /// A key published as the mailer's, which tests open what's sealed to
    /// it with.
    pub struct TestKey(age::x25519::Identity);

    impl TestKey {
        pub fn generate() -> Self {
            Self(age::x25519::Identity::generate())
        }

        pub async fn publish(outbox: &Outbox) -> Self {
            let key = Self::generate();
            outbox.publish_key(&key.public()).await.unwrap();
            key
        }

        pub fn public(&self) -> PublicKey {
            self.0.to_public()
        }

        /// `messages`, with what's sealed to this key opened.
        pub fn open(&self, messages: Vec<QueuedMessage>) -> Vec<QueuedMessage> {
            messages
                .into_iter()
                .map(|message| match message.body {
                    Body::Sealed { ciphertext, .. } => QueuedMessage {
                        body: Body::Plain(self.open_body(&ciphertext)),
                        ..message
                    },
                    Body::Plain(_) => message,
                })
                .collect()
        }

        fn open_body(&self, ciphertext: &[u8]) -> String {
            String::from_utf8(age::decrypt(&self.0, ciphertext).unwrap()).unwrap()
        }
    }

    impl QueuedMessage {
        /// What the message says, once it's plain or opened.
        pub fn text(&self) -> &str {
            match &self.body {
                Body::Plain(text) => text,
                Body::Sealed { .. } => panic!("message {} is sealed", self.id),
            }
        }
    }

    /// alice's verification email, then bob's a minute later.
    pub async fn with_outbox() -> (Outbox, tempfile::TempDir) {
        let (outbox, dir) = temp_outbox().await;
        for (account, queued_at) in [("alice", QUEUED), ("bob", QUEUED + 60)] {
            let to = format!("{account}@example.org");
            let message = message(Some(account), &to, VERIFY_SUBJECT);
            outbox.queue(&message, queued_at).await.unwrap();
        }
        (outbox, dir)
    }
}

#[cfg(test)]
mod tests {
    use super::{testing::*, *};
    use common::sqlite::testing::files_contain;

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
    fn a_messages_debug_output_leaves_the_body_out() {
        let message = message(Some("bob"), "bob@example.org", "Verify your email");
        let debug = format!("{message:?}");
        assert!(!debug.contains("q7Hx2mZp9vLcT4wRfBa"), "{debug}");
        assert!(debug.contains("body: <redacted>"), "{debug}");
        assert!(debug.contains("bob@example.org"), "{debug}");
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
    async fn the_mailers_newest_key_replaces_the_last() {
        let (outbox, _dir) = temp_outbox().await;
        assert_eq!(outbox.published_key().await.unwrap(), None);
        let [first, second] = [(); 2].map(|()| TestKey::generate().public());

        outbox.publish_key(&first).await.unwrap();
        assert_eq!(outbox.published_key().await.unwrap().as_ref(), Some(&first));
        outbox.publish_key(&second).await.unwrap();

        assert_eq!(outbox.published_key().await.unwrap(), Some(second));
    }

    const CODE: &[u8] = b"q7Hx2mZp9vLcT4wRfBa";

    #[tokio::test]
    async fn a_sealed_message_reaches_the_disk_sealed_to_the_published_key() {
        let (outbox, dir) = temp_outbox().await;
        let key = TestKey::publish(&outbox).await;
        let alices = message(Some("alice"), "alice@example.org", VERIFY_SUBJECT);

        outbox
            .queue_sealed(&alices, QUEUED, NO_SOONER)
            .await
            .unwrap();

        let due = outbox.due(QUEUED).await.unwrap();
        let Body::Sealed { to, .. } = &due[0].body else {
            panic!("sealed: {due:?}");
        };
        assert_eq!(*to, key.public().to_string());
        assert_eq!(due[0].recipient, "alice@example.org");
        assert_eq!(key.open(due)[0].text(), alices.body);
        sqlx::query("PRAGMA wal_checkpoint(TRUNCATE)")
            .execute(&outbox.pool)
            .await
            .unwrap();
        assert!(!files_contain(&dir, CODE));
        assert!(!files_contain(&dir, "Zoë".as_bytes()));
        assert!(files_contain(&dir, b"alice@example.org"));
    }

    #[tokio::test]
    async fn nothing_is_sealed_or_queued_before_the_mailer_publishes_a_key() {
        let (outbox, dir) = temp_outbox().await;
        let alices = message(Some("alice"), "alice@example.org", VERIFY_SUBJECT);

        let error = outbox
            .queue_sealed(&alices, QUEUED, NO_SOONER)
            .await
            .unwrap_err();

        assert_eq!(error.downcast_ref(), Some(&NoKey));
        assert!(outbox.due(QUEUED).await.unwrap().is_empty());
        assert!(!files_contain(&dir, CODE));
    }

    #[tokio::test]
    async fn a_new_key_deletes_what_was_sealed_to_the_old_but_not_plain_mail() {
        let (outbox, _dir) = with_outbox().await;
        let old = TestKey::publish(&outbox).await;
        let carols = message(Some("carol"), "carol@example.org", VERIFY_SUBJECT);
        outbox
            .queue_sealed(&carols, QUEUED, NO_SOONER)
            .await
            .unwrap();
        let new = TestKey::generate().public();

        let deletion = outbox.publish_key(&new).await.unwrap();

        assert_eq!(
            deletion,
            Deletion {
                messages: 1,
                off_the_disk: true,
            }
        );
        let due = outbox.due(QUEUED + 60).await.unwrap();
        assert_eq!(recipients(&due), ["alice@example.org", "bob@example.org"]);
        outbox
            .queue_sealed(&carols, QUEUED, NO_SOONER)
            .await
            .unwrap();
        assert_eq!(
            outbox
                .delete_sealed_to_others(&old.public())
                .await
                .unwrap()
                .messages,
            1
        );
        outbox
            .queue_sealed(&carols, QUEUED, NO_SOONER)
            .await
            .unwrap();
        assert_eq!(
            outbox.delete_sealed_to_others(&new).await.unwrap().messages,
            0
        );
        assert_eq!(outbox.due(QUEUED + 60).await.unwrap().len(), 3);
    }

    #[tokio::test]
    async fn withdrawing_the_key_deletes_all_sealed_mail_and_stops_sealing() {
        let (outbox, _dir) = with_outbox().await;
        TestKey::publish(&outbox).await;
        let carols = message(Some("carol"), "carol@example.org", VERIFY_SUBJECT);
        outbox
            .queue_sealed(&carols, QUEUED, NO_SOONER)
            .await
            .unwrap();

        let deletion = outbox.withdraw_key().await.unwrap();

        assert_eq!(
            deletion,
            Deletion {
                messages: 1,
                off_the_disk: true,
            }
        );
        assert_eq!(outbox.published_key().await.unwrap(), None);
        let due = outbox.due(QUEUED + 60).await.unwrap();
        assert_eq!(recipients(&due), ["alice@example.org", "bob@example.org"]);
        let error = outbox
            .queue_sealed(&carols, QUEUED, NO_SOONER)
            .await
            .unwrap_err();
        assert_eq!(error.downcast_ref(), Some(&NoKey));
        assert_eq!(outbox.withdraw_key().await.unwrap().messages, 0);
    }

    #[tokio::test]
    async fn only_sealed_mail_deleted_unsent_is_noted_unsent_until_taken() {
        let (outbox, _dir) = with_outbox().await;
        let alice = outbox.due(QUEUED).await.unwrap().remove(0);
        outbox.delete(alice.id).await.unwrap();
        TestKey::publish(&outbox).await;
        let carols = message(Some("carol"), "carol@example.org", VERIFY_SUBJECT);
        let mut sealed = Vec::new();
        for publish in [true, false] {
            sealed.push(
                outbox
                    .queue_sealed(&carols, QUEUED, NO_SOONER)
                    .await
                    .unwrap(),
            );
            if publish {
                outbox
                    .publish_key(&TestKey::generate().public())
                    .await
                    .unwrap();
            } else {
                outbox.withdraw_key().await.unwrap();
            }
        }
        TestKey::publish(&outbox).await;
        sealed.push(
            outbox
                .queue_sealed(&carols, QUEUED, NO_SOONER)
                .await
                .unwrap(),
        );
        let other = TestKey::generate().public();
        outbox.delete_sealed_to_others(&other).await.unwrap();

        assert_eq!(outbox.take_unsent().await.unwrap(), sealed);
        assert!(outbox.take_unsent().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn a_sealed_message_that_can_never_be_sent_is_noted_unsent_and_a_plain_one_is_not() {
        let (outbox, _dir) = with_outbox().await;
        TestKey::publish(&outbox).await;
        let carols = message(Some("carol"), "carol@example.org", VERIFY_SUBJECT);
        let sealed = outbox
            .queue_sealed(&carols, QUEUED, NO_SOONER)
            .await
            .unwrap();
        let alice = outbox.due(QUEUED).await.unwrap().remove(0);

        for id in [sealed, alice.id] {
            assert_eq!(outbox.delete_unsent(id).await.unwrap().messages, 1);
        }

        assert_eq!(outbox.take_unsent().await.unwrap(), [sealed]);
        assert_eq!(
            recipients(&outbox.due(QUEUED + 60).await.unwrap()),
            ["bob@example.org"]
        );
    }

    #[tokio::test]
    async fn sealing_waits_for_a_key_being_published_and_seals_to_it() {
        let (outbox, _dir) = temp_outbox().await;
        TestKey::publish(&outbox).await;
        let new = TestKey::generate();
        let mut publishing = outbox.pool.begin_with("BEGIN IMMEDIATE").await.unwrap();
        let key = new.public().to_string();
        sqlx::query("UPDATE mailer_key SET public_key = ?")
            .bind(&key)
            .execute(&mut *publishing)
            .await
            .unwrap();
        let alices = message(Some("alice"), "alice@example.org", VERIFY_SUBJECT);
        let sealing = {
            let outbox = outbox.clone();
            tokio::spawn(async move { outbox.queue_sealed(&alices, QUEUED, NO_SOONER).await })
        };
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;

        publishing.commit().await.unwrap();
        sealing.await.unwrap().unwrap();

        let due = outbox.due(QUEUED).await.unwrap();
        assert!(
            matches!(&due[0].body, Body::Sealed { to, .. } if *to == key),
            "{due:?}"
        );
        assert!(new.open(due)[0].text().starts_with("Hi alice,"));
    }

    #[tokio::test]
    async fn an_id_names_one_message_even_once_it_is_deleted() {
        let (outbox, _dir) = temp_outbox().await;
        let alices = message(Some("alice"), "alice@example.org", VERIFY_SUBJECT);
        let first = outbox.queue(&alices, QUEUED).await.unwrap();
        outbox.delete(first).await.unwrap();

        let second = outbox.queue(&alices, QUEUED).await.unwrap();

        assert!(second > first, "{second} after {first}");
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
                body: Body::Plain(message(Some("alice"), "alice@example.org", "-").body),
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
    async fn a_sealed_message_is_given_up_on_when_its_secret_stops_working() {
        let (outbox, _dir) = with_outbox().await;
        TestKey::publish(&outbox).await;
        let carols = message(Some("carol"), "carol@example.org", VERIFY_SUBJECT);
        let lapses_at = QUEUED + HOUR;
        outbox
            .queue_sealed(&carols, QUEUED + 60, lapses_at)
            .await
            .unwrap();
        let later = message(Some("dave"), "dave@example.org", VERIFY_SUBJECT);
        let never_sooner = QUEUED + GIVE_UP_AFTER + DAY;
        outbox
            .queue_sealed(&later, QUEUED + 60, never_sooner)
            .await
            .unwrap();
        let all = [
            "alice@example.org",
            "bob@example.org",
            "carol@example.org",
            "dave@example.org",
        ];
        assert_eq!(recipients(&outbox.due(lapses_at - 1).await.unwrap()), all);
        assert_eq!(
            outbox
                .delete_given_up(lapses_at - 1)
                .await
                .unwrap()
                .messages,
            0
        );

        let due = outbox.due(lapses_at).await.unwrap();
        let deletion = outbox.delete_given_up(lapses_at).await.unwrap();

        assert_eq!(
            recipients(&due),
            ["alice@example.org", "bob@example.org", "dave@example.org"]
        );
        assert_eq!(deletion.messages, 1);
        let daves_end = QUEUED + 60 + GIVE_UP_AFTER + 1;
        assert!(outbox.due(daves_end).await.unwrap().is_empty());
        assert_eq!(outbox.delete_given_up(daves_end).await.unwrap().messages, 3);
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
