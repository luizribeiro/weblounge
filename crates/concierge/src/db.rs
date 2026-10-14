use std::{
    path::Path,
    time::{Duration, SystemTime},
};

use anyhow::Context;
use sqlx::{
    SqliteExecutor, SqlitePool,
    migrate::Migrator,
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous},
};

static MIGRATOR: Migrator = sqlx::migrate!();
const BUSY_TIMEOUT: Duration = Duration::from_secs(5);

/// The current time as the database stores times, in Unix seconds.
pub fn now() -> i64 {
    let since_epoch = SystemTime::UNIX_EPOCH
        .elapsed()
        .expect("the clock is past 1970");
    since_epoch
        .as_secs()
        .try_into()
        .expect("the clock is before 2262")
}

pub struct NewMember<'a> {
    /// Ergo's canonical spelling of the account name.
    pub account: &'a str,
    pub email: Option<&'a str>,
    /// `None` for a root of the invite tree.
    pub invited_by: Option<&'a str>,
    pub joined_at: i64,
    pub invite_quota: i64,
    pub is_admin: bool,
}

/// A member, or the placeholder a removed member leaves in the invite tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Member {
    pub account: String,
    pub email: Option<String>,
    pub email_verified_at: Option<i64>,
    pub news_opt_in: bool,
    pub invited_by: Option<String>,
    pub joined_at: i64,
    pub invite_quota: i64,
    pub is_admin: bool,
    pub removed_at: Option<i64>,
}

/// A member's name, and whether only their placeholder is left.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemberAccount {
    pub account: String,
    pub removed: bool,
}

pub struct NewSession<'a> {
    pub token_hash: &'a [u8],
    pub account: &'a str,
    pub created_at: i64,
    pub expires_at: i64,
}

pub struct NewInvite<'a> {
    pub code: &'a str,
    /// `None` for a root invite.
    pub created_by: Option<&'a str>,
    pub note: Option<&'a str>,
    pub created_at: i64,
    pub expires_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Invite {
    pub code: String,
    pub created_by: Option<String>,
    pub note: Option<String>,
    pub created_at: i64,
    pub expires_at: i64,
    pub claimed_at: Option<i64>,
    pub redeemed_by: Option<String>,
    pub redeemed_at: Option<i64>,
    pub revoked_at: Option<i64>,
}

/// A session that hasn't expired, of a member who hasn't been removed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Session {
    pub member: Member,
    pub expires_at: i64,
}

/// What [`Db::remove_member`] did besides clearing the member's data.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Removal {
    pub sessions_ended: u64,
    pub invites_revoked: u64,
}

/// Concierge's SQLite database, migrated to the schema this build knows.
#[derive(Clone)]
pub struct Db {
    pool: SqlitePool,
}

impl Db {
    /// Opens the database at `path`, creating it if needed, and applies any
    /// migrations it lacks.
    pub async fn open(path: &Path) -> anyhow::Result<Self> {
        let options = SqliteConnectOptions::new()
            .filename(path)
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal)
            .synchronous(SqliteSynchronous::Normal)
            .foreign_keys(true)
            .busy_timeout(BUSY_TIMEOUT);
        let pool = SqlitePoolOptions::new()
            .connect_with(options)
            .await
            .with_context(|| format!("can't open the database {}", path.display()))?;
        MIGRATOR
            .run(&pool)
            .await
            .with_context(|| format!("can't migrate the database {}", path.display()))?;
        Ok(Self { pool })
    }

    /// The newest migration applied, if any.
    pub async fn migration_version(&self) -> sqlx::Result<Option<i64>> {
        sqlx::query_scalar!(
            r#"SELECT MAX(version) AS "version?" FROM _sqlx_migrations WHERE success"#
        )
        .fetch_one(&self.pool)
        .await
    }

    pub async fn insert_member(&self, member: &NewMember<'_>) -> sqlx::Result<()> {
        sqlx::query!(
            "INSERT INTO members (account, email, invited_by, joined_at, invite_quota, is_admin)
             VALUES (?, ?, ?, ?, ?, ?)",
            member.account,
            member.email,
            member.invited_by,
            member.joined_at,
            member.invite_quota,
            member.is_admin,
        )
        .execute(&self.pool)
        .await
        .map(drop)
    }

    /// The member whose account is `account`, in any case.
    pub async fn member(&self, account: &str) -> sqlx::Result<Option<Member>> {
        sqlx::query_as!(
            Member,
            r#"SELECT account AS "account!", email, email_verified_at, news_opt_in,
                      invited_by, joined_at, invite_quota, is_admin, removed_at
               FROM members WHERE account = ?"#,
            account
        )
        .fetch_optional(&self.pool)
        .await
    }

    /// Raises a current member's invite quota by `extra`, answering the new
    /// quota, or `None` if there's no such member or they were removed.
    pub async fn grant_invites(&self, account: &str, extra: i64) -> sqlx::Result<Option<i64>> {
        sqlx::query_scalar!(
            "UPDATE members SET invite_quota = invite_quota + ?
             WHERE account = ? AND removed_at IS NULL
             RETURNING invite_quota",
            extra,
            account,
        )
        .fetch_optional(&self.pool)
        .await
    }

    /// Sets a member's email and news consent. A changed address is no
    /// longer verified.
    pub async fn update_settings(
        &self,
        account: &str,
        email: &str,
        news_opt_in: bool,
    ) -> sqlx::Result<()> {
        sqlx::query!(
            "UPDATE members
             SET email_verified_at = CASE WHEN email IS ? THEN email_verified_at END,
                 email = ?, news_opt_in = ?
             WHERE account = ? AND removed_at IS NULL",
            email,
            email,
            news_opt_in,
            account,
        )
        .execute(&self.pool)
        .await
        .map(drop)
    }

    /// Every member's account, removed ones included.
    pub async fn member_accounts(&self) -> sqlx::Result<Vec<MemberAccount>> {
        sqlx::query_as!(
            MemberAccount,
            r#"SELECT account AS "account!", removed_at IS NOT NULL AS "removed!: bool"
               FROM members ORDER BY account"#
        )
        .fetch_all(&self.pool)
        .await
    }

    pub async fn insert_session(&self, session: &NewSession<'_>) -> sqlx::Result<()> {
        sqlx::query!(
            "INSERT INTO sessions (token_hash, account, created_at, expires_at) VALUES (?, ?, ?, ?)",
            session.token_hash,
            session.account,
            session.created_at,
            session.expires_at,
        )
        .execute(&self.pool)
        .await
        .map(drop)
    }

    /// The live session whose token hashes to `token_hash`, if any.
    pub async fn session(&self, token_hash: &[u8], now: i64) -> sqlx::Result<Option<Session>> {
        let row = sqlx::query!(
            "SELECT account, expires_at FROM sessions WHERE token_hash = ? AND expires_at > ?",
            token_hash,
            now,
        )
        .fetch_optional(&self.pool)
        .await?;
        let Some(row) = row else {
            return Ok(None);
        };
        let member = self.member(&row.account).await?;
        Ok(member
            .filter(|member| member.removed_at.is_none())
            .map(|member| Session {
                member,
                expires_at: row.expires_at,
            }))
    }

    pub async fn renew_session(&self, token_hash: &[u8], expires_at: i64) -> sqlx::Result<()> {
        sqlx::query!(
            "UPDATE sessions SET expires_at = ? WHERE token_hash = ?",
            expires_at,
            token_hash,
        )
        .execute(&self.pool)
        .await
        .map(drop)
    }

    pub async fn end_session(&self, token_hash: &[u8]) -> sqlx::Result<()> {
        sqlx::query!("DELETE FROM sessions WHERE token_hash = ?", token_hash)
            .execute(&self.pool)
            .await
            .map(drop)
    }

    /// Deletes the sessions that expired by `now`, answering how many.
    pub async fn end_expired_sessions(&self, now: i64) -> sqlx::Result<u64> {
        let deleted = sqlx::query!("DELETE FROM sessions WHERE expires_at <= ?", now)
            .execute(&self.pool)
            .await?;
        Ok(deleted.rows_affected())
    }

    /// Ends every session of `account`, answering how many there were.
    pub async fn end_sessions(&self, account: &str) -> sqlx::Result<u64> {
        delete_sessions(&self.pool, account).await
    }

    /// Clears everything personal about a member, ends their sessions and
    /// revokes their open invites. Their row stays as their place in the
    /// invite tree, so the people they invited stay attached.
    pub async fn remove_member(&self, account: &str, now: i64) -> sqlx::Result<Removal> {
        let mut transaction = self.pool.begin().await?;
        sqlx::query!(
            "UPDATE members
             SET email = NULL, email_verified_at = NULL, news_opt_in = FALSE,
                 invite_quota = 0, is_admin = FALSE, removed_at = ?
             WHERE account = ?",
            now,
            account,
        )
        .execute(&mut *transaction)
        .await?;
        let sessions_ended = delete_sessions(&mut *transaction, account).await?;
        let revoked = sqlx::query!(
            "UPDATE invites SET revoked_at = ?
             WHERE created_by = ? AND redeemed_at IS NULL AND revoked_at IS NULL
               AND expires_at > ?",
            now,
            account,
            now,
        )
        .execute(&mut *transaction)
        .await?;
        // Notes are written about the invitee, so the ones on the invite
        // that brought the member in are about them too.
        sqlx::query!(
            "UPDATE invites SET note = NULL WHERE created_by = ? OR redeemed_by = ?",
            account,
            account,
        )
        .execute(&mut *transaction)
        .await?;
        transaction.commit().await?;
        Ok(Removal {
            sessions_ended,
            invites_revoked: revoked.rows_affected(),
        })
    }

    pub async fn insert_invite(&self, invite: &NewInvite<'_>) -> sqlx::Result<()> {
        sqlx::query!(
            "INSERT INTO invites (code, created_by, note, created_at, expires_at)
             VALUES (?, ?, ?, ?, ?)",
            invite.code,
            invite.created_by,
            invite.note,
            invite.created_at,
            invite.expires_at,
        )
        .execute(&self.pool)
        .await
        .map(drop)
    }

    pub async fn invite(&self, code: &str) -> sqlx::Result<Option<Invite>> {
        sqlx::query_as!(
            Invite,
            r#"SELECT code AS "code!", created_by, note, created_at, expires_at, claimed_at,
                      redeemed_by, redeemed_at, revoked_at
               FROM invites WHERE code = ?"#,
            code
        )
        .fetch_optional(&self.pool)
        .await
    }

    /// Members who haven't been removed.
    pub async fn member_count(&self) -> sqlx::Result<i64> {
        sqlx::query_scalar!(
            r#"SELECT COUNT(*) AS "count!: i64" FROM members WHERE removed_at IS NULL"#
        )
        .fetch_one(&self.pool)
        .await
    }
}

async fn delete_sessions(executor: impl SqliteExecutor<'_>, account: &str) -> sqlx::Result<u64> {
    let deleted = sqlx::query!("DELETE FROM sessions WHERE account = ?", account)
        .execute(executor)
        .await?;
    Ok(deleted.rows_affected())
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub async fn temp_db() -> (Db, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open(&dir.path().join("concierge.db")).await.unwrap();
        (db, dir)
    }

    async fn execute(db: &Db, sql: &str) -> sqlx::Result<()> {
        sqlx::query(sql).execute(&db.pool).await.map(drop)
    }

    fn latest_migration() -> i64 {
        MIGRATOR
            .iter()
            .map(|migration| migration.version)
            .max()
            .unwrap()
    }

    #[tokio::test]
    async fn applies_every_migration_once() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("concierge.db");
        Db::open(&path).await.unwrap();
        let reopened = Db::open(&path).await.unwrap();
        assert_eq!(
            reopened.migration_version().await.unwrap(),
            Some(latest_migration())
        );
    }

    #[tokio::test]
    async fn writes_ahead_and_enforces_foreign_keys() {
        let (db, _dir) = temp_db().await;
        let mode: String = sqlx::query_scalar("PRAGMA journal_mode")
            .fetch_one(&db.pool)
            .await
            .unwrap();
        assert_eq!(mode, "wal");
        let error = execute(
            &db,
            "INSERT INTO invites (code, created_by, created_at, expires_at) VALUES ('c', 'nobody', 0, 1)",
        )
        .await
        .unwrap_err();
        assert!(error.to_string().contains("FOREIGN KEY"), "{error}");
    }

    #[tokio::test]
    async fn account_names_are_unique_whatever_their_case() {
        let (db, _dir) = temp_db().await;
        let insert = |name: &str| {
            format!(
                "INSERT INTO members (account, joined_at, invite_quota) VALUES ('{name}', 0, 0)"
            )
        };
        execute(&db, &insert("Alice")).await.unwrap();
        let error = execute(&db, &insert("aLICE")).await.unwrap_err();
        assert!(error.to_string().contains("UNIQUE"), "{error}");
    }

    #[tokio::test]
    async fn a_removed_member_keeps_nothing_personal() {
        let (db, _dir) = temp_db().await;
        let error = execute(
            &db,
            "INSERT INTO members (account, email, joined_at, invite_quota, removed_at)
             VALUES ('alice', 'alice@example.org', 0, 0, 1)",
        )
        .await
        .unwrap_err();
        assert!(error.to_string().contains("CHECK"), "{error}");
        execute(
            &db,
            "INSERT INTO members (account, joined_at, invite_quota, removed_at) VALUES ('alice', 0, 0, 1)",
        )
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn removing_a_member_keeps_the_invites_that_shaped_the_tree() {
        let (db, _dir) = temp_db().await;
        execute(
            &db,
            "INSERT INTO members (account, email, invited_by, joined_at, invite_quota)
             VALUES ('root', NULL, NULL, 0, 5),
                    ('alice', 'alice@example.org', 'root', 10, 5),
                    ('bob', 'bob@example.org', 'alice', 20, 5);
             UPDATE members SET email_verified_at = 11, news_opt_in = TRUE, is_admin = TRUE
             WHERE account = 'alice';
             INSERT INTO invites (code, created_by, note, created_at, expires_at, redeemed_by, redeemed_at)
             VALUES ('to-alice', 'root', 'for alice from work', 5, 100, 'alice', 10),
                    ('to-bob', 'alice', 'for bob', 15, 100, 'bob', 20),
                    ('stale', 'alice', 'for carol', 15, 100, NULL, NULL),
                    ('open', 'alice', 'for dave', 150, 300, NULL, NULL);",
        )
        .await
        .unwrap();

        let removal = db.remove_member("alice", 200).await.unwrap();

        assert_eq!(removal.invites_revoked, 1);
        let alice = db.member("alice").await.unwrap().unwrap();
        assert_eq!(
            alice,
            Member {
                account: "alice".into(),
                email: None,
                email_verified_at: None,
                news_opt_in: false,
                invited_by: Some("root".into()),
                joined_at: 10,
                invite_quota: 0,
                is_admin: false,
                removed_at: Some(200),
            }
        );
        for (code, redeemed_by, revoked_at) in [
            ("open", None, Some(200)),
            ("stale", None, None),
            ("to-alice", Some("alice"), None),
            ("to-bob", Some("bob"), None),
        ] {
            let invite = db.invite(code).await.unwrap().unwrap();
            assert_eq!(invite.note, None, "{code}");
            assert_eq!(invite.redeemed_by.as_deref(), redeemed_by, "{code}");
            assert_eq!(invite.revoked_at, revoked_at, "{code}");
        }
        let bob = db.member("bob").await.unwrap().unwrap();
        assert_eq!(bob.invited_by.as_deref(), Some("alice"));
        assert_eq!(bob.email.as_deref(), Some("bob@example.org"));
    }

    async fn with_sessions() -> (Db, tempfile::TempDir) {
        let (db, dir) = temp_db().await;
        execute(
            &db,
            "INSERT INTO members (account, joined_at, invite_quota) VALUES ('alice', 0, 5), ('bob', 0, 5);
             INSERT INTO sessions (token_hash, account, created_at, expires_at)
             VALUES (x'a1', 'alice', 0, 100), (x'a2', 'alice', 0, 200), (x'b1', 'bob', 0, 100);",
        )
        .await
        .unwrap();
        (db, dir)
    }

    #[tokio::test]
    async fn a_session_signs_its_member_in_until_it_expires() {
        let (db, _dir) = with_sessions().await;
        let session = db.session(b"\xa1", 99).await.unwrap().unwrap();
        assert_eq!(
            (session.member.account.as_str(), session.expires_at),
            ("alice", 100)
        );
        assert_eq!(db.session(b"\xa1", 100).await.unwrap(), None);
        assert_eq!(db.session(b"\xff", 0).await.unwrap(), None);
    }

    #[tokio::test]
    async fn renewing_pushes_the_expiry_back() {
        let (db, _dir) = with_sessions().await;
        db.renew_session(b"\xa1", 500).await.unwrap();
        assert_eq!(
            db.session(b"\xa1", 300).await.unwrap().unwrap().expires_at,
            500
        );
        assert_eq!(
            db.session(b"\xb1", 0).await.unwrap().unwrap().expires_at,
            100
        );
    }

    #[tokio::test]
    async fn ending_a_session_leaves_the_members_others() {
        let (db, _dir) = with_sessions().await;
        db.end_session(b"\xa1").await.unwrap();
        assert_eq!(db.session(b"\xa1", 0).await.unwrap(), None);
        assert!(db.session(b"\xa2", 0).await.unwrap().is_some());
    }

    #[tokio::test]
    async fn expired_sessions_are_cleaned_up() {
        let (db, _dir) = with_sessions().await;
        assert_eq!(db.end_expired_sessions(150).await.unwrap(), 2);
        assert!(db.session(b"\xa2", 150).await.unwrap().is_some());
        assert_eq!(db.end_sessions("bob").await.unwrap(), 0);
    }

    #[tokio::test]
    async fn changing_the_email_drops_its_verification_but_keeping_it_does_not() {
        let (db, _dir) = temp_db().await;
        execute(
            &db,
            "INSERT INTO members (account, email, email_verified_at, joined_at, invite_quota)
             VALUES ('alice', 'alice@example.org', 10, 0, 5)",
        )
        .await
        .unwrap();
        db.update_settings("alice", "alice@example.org", true)
            .await
            .unwrap();
        let alice = db.member("alice").await.unwrap().unwrap();
        assert_eq!(
            (alice.email_verified_at, alice.news_opt_in),
            (Some(10), true)
        );
        db.update_settings("alice", "alice@example.net", false)
            .await
            .unwrap();
        let alice = db.member("alice").await.unwrap().unwrap();
        assert_eq!(alice.email.as_deref(), Some("alice@example.net"));
        assert_eq!((alice.email_verified_at, alice.news_opt_in), (None, false));
    }

    #[tokio::test]
    async fn a_removed_members_session_signs_nobody_in() {
        let (db, _dir) = with_sessions().await;
        execute(
            &db,
            "UPDATE members SET invite_quota = 0, removed_at = 1 WHERE account = 'alice'",
        )
        .await
        .unwrap();
        assert_eq!(db.session(b"\xa2", 0).await.unwrap(), None);
    }
}
