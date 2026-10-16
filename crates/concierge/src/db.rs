use std::{
    path::Path,
    time::{Duration, SystemTime},
};

use anyhow::Context;
use sqlx::{
    SqliteConnection, SqliteExecutor, SqlitePool,
    migrate::Migrator,
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous},
};

use crate::{invite_code, invite_rules, outbox, tree::Branch};

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
    pub news_opt_in: bool,
    /// `None` for a root of the invite tree.
    pub invited_by: Option<&'a str>,
    pub joined_at: i64,
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

pub struct NewAuthorizationCode<'a> {
    pub code_hash: &'a [u8],
    /// The session that approved it, whose member it signs in.
    pub session_hash: &'a [u8],
    pub client_id: &'a str,
    pub redirect_uri: &'a str,
    pub code_challenge: &'a str,
    pub expires_at: i64,
}

/// An authorization code, taken out of the database by its one use.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthorizationCode {
    pub session_hash: Vec<u8>,
    pub client_id: String,
    pub redirect_uri: String,
    pub code_challenge: String,
    pub expires_at: i64,
}

pub struct NewAccessToken<'a> {
    pub token_hash: &'a [u8],
    pub session_hash: &'a [u8],
    pub client_id: &'a str,
    pub created_at: i64,
    pub expires_at: i64,
}

/// A live access token, of a member who hasn't been removed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccessToken {
    pub account: String,
    pub client_id: String,
    pub expires_at: i64,
}

pub struct NewInvite<'a> {
    /// The code itself, which only its hash is kept of.
    pub code: &'a str,
    /// `None` for a root invite.
    pub created_by: Option<&'a str>,
    pub created_at: i64,
    pub expires_at: i64,
    /// Whether whoever redeems it becomes an admin.
    pub makes_admin: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Invite {
    pub code_hash: Vec<u8>,
    pub created_by: Option<String>,
    pub created_at: i64,
    pub expires_at: i64,
    pub claimed_at: Option<i64>,
    pub redeemed_by: Option<String>,
    pub redeemed_at: Option<i64>,
    pub revoked_at: Option<i64>,
    pub makes_admin: bool,
}

impl Invite {
    /// What the invite rules need to know about it.
    pub fn rules(&self) -> invite_rules::Invite {
        invite_rules::Invite {
            expires_at: self.expires_at,
            claimed_at: self.claimed_at,
            redeemed_at: self.redeemed_at,
            revoked_at: self.revoked_at,
        }
    }

    pub fn status(&self, now: i64) -> invite_rules::Status {
        self.rules().status(now)
    }
}

/// An invite held for one redemption, from [`Db::claim_invite`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Claim {
    pub code_hash: Vec<u8>,
    pub claimed_at: i64,
    pub created_by: Option<String>,
    pub makes_admin: bool,
}

/// Someone a member invited.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Invitee {
    pub account: String,
    pub joined_at: i64,
    pub removed: bool,
}

/// What came of [`Db::redeem_invite`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Redemption {
    Redeemed,
    /// The invite was revoked, or the claim went stale and another
    /// redemption took it over; nothing was written.
    Lost,
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
    /// Whether the overwritten data left the disk now, rather than at the
    /// next vacuum.
    pub off_the_disk: bool,
}

pub struct NewBot<'a> {
    /// Ergo's canonical spelling of the account name.
    pub account: &'a str,
    pub owner: &'a str,
    pub note: Option<&'a str>,
    pub created_at: i64,
}

/// A bot, or the row a deleted bot leaves to keep its name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bot {
    pub account: String,
    pub owner: String,
    pub note: Option<String>,
    pub created_at: i64,
    pub removed_at: Option<i64>,
}

/// A bot's name and owner, and which of the two is gone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BotAccount {
    pub account: String,
    pub owner: String,
    pub removed: bool,
    pub owner_removed: bool,
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
            .pragma("secure_delete", "ON")
            .busy_timeout(BUSY_TIMEOUT);
        let pool = SqlitePoolOptions::new()
            .connect_with(options)
            .await
            .with_context(|| format!("can't open the database {}", path.display()))?;
        migrate(&pool)
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
        insert_member(&self.pool, member).await
    }

    /// The member whose account is `account`, in any case.
    pub async fn member(&self, account: &str) -> sqlx::Result<Option<Member>> {
        sqlx::query_as!(
            Member,
            r#"SELECT account AS "account!", email, email_verified_at, news_opt_in,
                      invited_by, joined_at, is_admin, removed_at
               FROM members WHERE account = ?"#,
            account
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

    /// Ends every session of `account` but the one whose token hashes to
    /// `kept`, answering how many it ended. Their OAuth codes and tokens go
    /// with them.
    pub async fn end_other_sessions(&self, account: &str, kept: &[u8]) -> sqlx::Result<u64> {
        let deleted = sqlx::query!(
            "DELETE FROM sessions WHERE account = ? AND token_hash != ?",
            account,
            kept,
        )
        .execute(&self.pool)
        .await?;
        Ok(deleted.rows_affected())
    }

    /// Clears everything personal about a member, mail waiting for them
    /// included, ends their sessions and revokes their open invites. Their
    /// row stays as their place in the invite tree, so the people they
    /// invited stay attached.
    pub async fn remove_member(&self, account: &str, now: i64) -> sqlx::Result<Removal> {
        let mut transaction = self.pool.begin().await?;
        sqlx::query!("DELETE FROM outbox WHERE account = ?", account)
            .execute(&mut *transaction)
            .await?;
        sqlx::query!(
            "UPDATE members
             SET email = NULL, email_verified_at = NULL, news_opt_in = FALSE,
                 is_admin = FALSE, removed_at = ?
             WHERE account = ?",
            now,
            account,
        )
        .execute(&mut *transaction)
        .await?;
        let sessions_ended = delete_sessions(&mut *transaction, account).await?;
        let invites_revoked = revoke_open_invites(&mut *transaction, account, now).await?;
        transaction.commit().await?;
        Ok(Removal {
            sessions_ended,
            invites_revoked,
            off_the_disk: self.empty_write_ahead_log().await,
        })
    }

    /// Copies the write-ahead log into the database and truncates it, so
    /// the rows a change overwrote leave the disk, not just the database.
    /// Answers whether it could, which a reader holding the log prevents.
    async fn empty_write_ahead_log(&self) -> bool {
        let checkpoint: sqlx::Result<(i64, i64, i64)> =
            sqlx::query_as("PRAGMA wal_checkpoint(TRUNCATE)")
                .fetch_one(&self.pool)
                .await;
        matches!(checkpoint, Ok((0, _, _)))
    }

    /// Revokes the invites `account` made that are still open, or being
    /// redeemed right now; answers how many.
    pub async fn revoke_open_invites(&self, account: &str, now: i64) -> sqlx::Result<u64> {
        revoke_open_invites(&self.pool, account, now).await
    }

    pub async fn insert_invite(&self, invite: &NewInvite<'_>) -> sqlx::Result<()> {
        insert_invite(&self.pool, invite).await
    }

    /// Inserts `invite` if `allowed` says so, given the invites its creator
    /// made already; answers whether it did. Concurrent calls take turns,
    /// so they can't both see room for one more.
    pub async fn insert_invite_if(
        &self,
        invite: &NewInvite<'_>,
        allowed: impl FnOnce(&[Invite]) -> bool,
    ) -> sqlx::Result<bool> {
        let mut transaction = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let invites = sqlx::query_as!(
            Invite,
            r#"SELECT code_hash AS "code_hash!", created_by, created_at, expires_at, claimed_at,
                      redeemed_by, redeemed_at, revoked_at, makes_admin
               FROM invites WHERE created_by = ?"#,
            invite.created_by,
        )
        .fetch_all(&mut *transaction)
        .await?;
        if !allowed(&invites) {
            return Ok(false);
        }
        insert_invite(&mut *transaction, invite).await?;
        transaction.commit().await?;
        Ok(true)
    }

    pub async fn invite(&self, code: &str) -> sqlx::Result<Option<Invite>> {
        let code_hash = invite_code::hash(code);
        sqlx::query_as!(
            Invite,
            r#"SELECT code_hash AS "code_hash!", created_by, created_at, expires_at, claimed_at,
                      redeemed_by, redeemed_at, revoked_at, makes_admin
               FROM invites WHERE code_hash = ?"#,
            code_hash
        )
        .fetch_optional(&self.pool)
        .await
    }

    /// The invites `account` created, newest first.
    pub async fn invites_created_by(&self, account: &str) -> sqlx::Result<Vec<Invite>> {
        sqlx::query_as!(
            Invite,
            r#"SELECT code_hash AS "code_hash!", created_by, created_at, expires_at, claimed_at,
                      redeemed_by, redeemed_at, revoked_at, makes_admin
               FROM invites WHERE created_by = ? ORDER BY created_at DESC, rowid DESC"#,
            account
        )
        .fetch_all(&self.pool)
        .await
    }

    /// The people `account` invited, in the order they joined.
    pub async fn invitees(&self, account: &str) -> sqlx::Result<Vec<Invitee>> {
        sqlx::query_as!(
            Invitee,
            r#"SELECT account AS "account!", joined_at, removed_at IS NOT NULL AS "removed!: bool"
               FROM members WHERE invited_by = ? ORDER BY joined_at, account"#,
            account
        )
        .fetch_all(&self.pool)
        .await
    }

    /// Every member's place in the invite tree, removed ones included.
    pub async fn invite_tree(&self) -> sqlx::Result<Vec<Branch>> {
        sqlx::query_as!(
            Branch,
            r#"SELECT account AS "account!", invited_by, joined_at,
                      removed_at IS NOT NULL AS "removed!: bool"
               FROM members"#
        )
        .fetch_all(&self.pool)
        .await
    }

    /// Holds the invite `code` for one redemption, if it's open now.
    pub async fn claim_invite(&self, code: &str, now: i64) -> sqlx::Result<Option<Claim>> {
        let Some(invite) = self.invite(code).await? else {
            return Ok(None);
        };
        if invite.status(now) != invite_rules::Status::Open {
            return Ok(None);
        }
        // Takes the invite only if nobody claimed, redeemed or revoked it
        // since it was read.
        let claimed = sqlx::query!(
            "UPDATE invites SET claimed_at = ?
             WHERE code_hash = ? AND claimed_at IS ? AND redeemed_at IS NULL AND revoked_at IS NULL",
            now,
            invite.code_hash,
            invite.claimed_at,
        )
        .execute(&self.pool)
        .await?;
        Ok((claimed.rows_affected() == 1).then_some(Claim {
            code_hash: invite.code_hash,
            claimed_at: now,
            created_by: invite.created_by,
            makes_admin: invite.makes_admin,
        }))
    }

    /// Gives a claimed invite back, unless another redemption took it over.
    pub async fn release_claim(&self, claim: &Claim) -> sqlx::Result<()> {
        sqlx::query!(
            "UPDATE invites SET claimed_at = NULL
             WHERE code_hash = ? AND claimed_at = ? AND redeemed_at IS NULL",
            claim.code_hash,
            claim.claimed_at,
        )
        .execute(&self.pool)
        .await
        .map(drop)
    }

    /// Adds `member` and marks the claimed invite redeemed by them, both or
    /// neither.
    pub async fn redeem_invite(
        &self,
        claim: &Claim,
        member: &NewMember<'_>,
    ) -> sqlx::Result<Redemption> {
        let mut transaction = self.pool.begin().await?;
        insert_member(&mut *transaction, member).await?;
        let redeemed = sqlx::query!(
            "UPDATE invites SET redeemed_by = ?, redeemed_at = ?
             WHERE code_hash = ? AND claimed_at = ? AND redeemed_at IS NULL AND revoked_at IS NULL",
            member.account,
            member.joined_at,
            claim.code_hash,
            claim.claimed_at,
        )
        .execute(&mut *transaction)
        .await?;
        if redeemed.rows_affected() == 0 {
            return Ok(Redemption::Lost);
        }
        transaction.commit().await?;
        Ok(Redemption::Redeemed)
    }

    /// Stores an authorization code, cleaning up expired ones while at it.
    pub async fn insert_authorization_code(
        &self,
        code: &NewAuthorizationCode<'_>,
        now: i64,
    ) -> sqlx::Result<()> {
        sqlx::query!("DELETE FROM oauth_codes WHERE expires_at <= ?", now)
            .execute(&self.pool)
            .await?;
        sqlx::query!(
            "INSERT INTO oauth_codes
                 (code_hash, session_hash, client_id, redirect_uri, code_challenge, expires_at)
             VALUES (?, ?, ?, ?, ?, ?)",
            code.code_hash,
            code.session_hash,
            code.client_id,
            code.redirect_uri,
            code.code_challenge,
            code.expires_at,
        )
        .execute(&self.pool)
        .await
        .map(drop)
    }

    /// Removes the code that hashes to `code_hash` and answers it, so no
    /// code is used twice, even by requests racing each other.
    pub async fn take_authorization_code(
        &self,
        code_hash: &[u8],
    ) -> sqlx::Result<Option<AuthorizationCode>> {
        sqlx::query_as!(
            AuthorizationCode,
            "DELETE FROM oauth_codes WHERE code_hash = ?
             RETURNING session_hash, client_id, redirect_uri, code_challenge, expires_at",
            code_hash
        )
        .fetch_optional(&self.pool)
        .await
    }

    /// Stores an access token, cleaning up expired ones while at it.
    pub async fn insert_access_token(&self, token: &NewAccessToken<'_>) -> sqlx::Result<()> {
        sqlx::query!(
            "DELETE FROM oauth_tokens WHERE expires_at <= ?",
            token.created_at
        )
        .execute(&self.pool)
        .await?;
        sqlx::query!(
            "INSERT INTO oauth_tokens (token_hash, session_hash, client_id, created_at, expires_at)
             VALUES (?, ?, ?, ?, ?)",
            token.token_hash,
            token.session_hash,
            token.client_id,
            token.created_at,
            token.expires_at,
        )
        .execute(&self.pool)
        .await
        .map(drop)
    }

    /// The live token that hashes to `token_hash`, while its session lasts
    /// and its member hasn't been removed.
    pub async fn access_token(
        &self,
        token_hash: &[u8],
        now: i64,
    ) -> sqlx::Result<Option<AccessToken>> {
        sqlx::query_as!(
            AccessToken,
            r#"SELECT members.account AS "account!", oauth_tokens.client_id,
                      oauth_tokens.expires_at
               FROM oauth_tokens
               JOIN sessions ON sessions.token_hash = oauth_tokens.session_hash
               JOIN members ON members.account = sessions.account
               WHERE oauth_tokens.token_hash = ? AND oauth_tokens.expires_at > ?
                 AND sessions.expires_at > ? AND members.removed_at IS NULL"#,
            token_hash,
            now,
            now,
        )
        .fetch_optional(&self.pool)
        .await
    }

    /// Whether a member or a bot holds `account`, in any case. Removed
    /// members and deleted bots keep their names, even once Ergo has erased
    /// their accounts and would hand the names out again.
    pub async fn name_held(&self, account: &str) -> sqlx::Result<bool> {
        sqlx::query_scalar!(
            r#"SELECT EXISTS (SELECT 1 FROM members WHERE account = ?1)
                   OR EXISTS (SELECT 1 FROM bots WHERE account = ?1) AS "held!: bool""#,
            account
        )
        .fetch_one(&self.pool)
        .await
    }

    pub async fn insert_bot(&self, bot: &NewBot<'_>) -> sqlx::Result<()> {
        sqlx::query!(
            "INSERT INTO bots (account, owner, note, created_at) VALUES (?, ?, ?, ?)",
            bot.account,
            bot.owner,
            bot.note,
            bot.created_at,
        )
        .execute(&self.pool)
        .await
        .map(drop)
    }

    /// The bot whose account is `account`, in any case, deleted or not.
    pub async fn bot(&self, account: &str) -> sqlx::Result<Option<Bot>> {
        sqlx::query_as!(
            Bot,
            r#"SELECT account AS "account!", owner, note, created_at, removed_at
               FROM bots WHERE account = ?"#,
            account
        )
        .fetch_optional(&self.pool)
        .await
    }

    /// The bots that haven't been deleted, of `owner` or of everyone, by
    /// name.
    pub async fn bots(&self, owner: Option<&str>) -> sqlx::Result<Vec<Bot>> {
        sqlx::query_as!(
            Bot,
            r#"SELECT account AS "account!", owner, note, created_at, removed_at
               FROM bots WHERE removed_at IS NULL AND (?1 IS NULL OR owner = ?1 COLLATE NOCASE)
               ORDER BY account"#,
            owner
        )
        .fetch_all(&self.pool)
        .await
    }

    /// Drops the row of a bot Ergo never created, freeing its name.
    pub async fn forget_bot(&self, account: &str) -> sqlx::Result<()> {
        sqlx::query!("DELETE FROM bots WHERE account = ?", account)
            .execute(&self.pool)
            .await
            .map(drop)
    }

    /// Every bot's account, deleted ones included.
    pub async fn bot_accounts(&self) -> sqlx::Result<Vec<BotAccount>> {
        sqlx::query_as!(
            BotAccount,
            r#"SELECT bots.account AS "account!", bots.owner,
                      bots.removed_at IS NOT NULL AS "removed!: bool",
                      members.removed_at IS NOT NULL AS "owner_removed!: bool"
               FROM bots JOIN members ON members.account = bots.owner
               ORDER BY bots.account"#
        )
        .fetch_all(&self.pool)
        .await
    }

    /// Marks a bot deleted, keeping its row so its name stays taken;
    /// answers whether it was live.
    pub async fn remove_bot(&self, account: &str, now: i64) -> sqlx::Result<bool> {
        let removed = sqlx::query!(
            "UPDATE bots SET removed_at = ? WHERE account = ? AND removed_at IS NULL",
            now,
            account,
        )
        .execute(&self.pool)
        .await?;
        Ok(removed.rows_affected() == 1)
    }

    /// Members who haven't been removed.
    pub async fn member_count(&self) -> sqlx::Result<i64> {
        sqlx::query_scalar!(
            r#"SELECT COUNT(*) AS "count!: i64" FROM members WHERE removed_at IS NULL"#
        )
        .fetch_one(&self.pool)
        .await
    }

    /// Leaves `message` for the mailer to send as soon as it can, and
    /// answers its id, which the mailer logs it by.
    pub async fn queue_message(&self, message: &outbox::Message, now: i64) -> sqlx::Result<i64> {
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
    pub async fn due_messages(&self, now: i64) -> sqlx::Result<Vec<QueuedMessage>> {
        let given_up_before = outbox::given_up_before(now);
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
        let next_attempt_at = outbox::next_attempt_at(message.attempts + 1, now);
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
    pub async fn delete_message(&self, id: i64) -> sqlx::Result<Deletion> {
        let deleted = sqlx::query!("DELETE FROM outbox WHERE id = ?", id)
            .execute(&self.pool)
            .await?;
        Ok(self.erase(deleted.rows_affected()).await)
    }

    /// Deletes the messages the mailer has given up on by `now`.
    pub async fn delete_given_up_messages(&self, now: i64) -> sqlx::Result<Deletion> {
        let given_up_before = outbox::given_up_before(now);
        let deleted = sqlx::query!("DELETE FROM outbox WHERE created_at < ?", given_up_before)
            .execute(&self.pool)
            .await?;
        Ok(self.erase(deleted.rows_affected()).await)
    }

    /// Takes the `messages` just deleted off the disk too.
    async fn erase(&self, messages: u64) -> Deletion {
        Deletion {
            messages,
            off_the_disk: messages == 0 || self.empty_write_ahead_log().await,
        }
    }
}

async fn insert_member(
    executor: impl SqliteExecutor<'_>,
    member: &NewMember<'_>,
) -> sqlx::Result<()> {
    sqlx::query!(
        "INSERT INTO members (account, email, news_opt_in, invited_by, joined_at, is_admin)
         VALUES (?, ?, ?, ?, ?, ?)",
        member.account,
        member.email,
        member.news_opt_in,
        member.invited_by,
        member.joined_at,
        member.is_admin,
    )
    .execute(executor)
    .await
    .map(drop)
}

async fn insert_invite(
    executor: impl SqliteExecutor<'_>,
    invite: &NewInvite<'_>,
) -> sqlx::Result<()> {
    let code_hash = invite_code::hash(invite.code);
    sqlx::query!(
        "INSERT INTO invites (code_hash, created_by, created_at, expires_at, makes_admin)
         VALUES (?, ?, ?, ?, ?)",
        code_hash,
        invite.created_by,
        invite.created_at,
        invite.expires_at,
        invite.makes_admin,
    )
    .execute(executor)
    .await
    .map(drop)
}

/// Applies the migrations `pool`'s database lacks with foreign keys off,
/// which SQLite needs for rebuilding a table others refer to, and then
/// checks that every reference still holds.
async fn migrate(pool: &SqlitePool) -> anyhow::Result<()> {
    let mut connection = pool.acquire().await?;
    sqlx::query("PRAGMA foreign_keys = OFF")
        .execute(&mut *connection)
        .await?;
    let migrated = migrate_and_check(&mut connection).await;
    sqlx::query("PRAGMA foreign_keys = ON")
        .execute(&mut *connection)
        .await?;
    migrated
}

async fn migrate_and_check(connection: &mut SqliteConnection) -> anyhow::Result<()> {
    MIGRATOR.run_direct(&mut *connection).await?;
    hash_carried_over_codes(&mut *connection).await?;
    let broken = sqlx::query("PRAGMA foreign_key_check")
        .fetch_all(&mut *connection)
        .await?;
    anyhow::ensure!(
        broken.is_empty(),
        "{} rows refer to rows that aren't there",
        broken.len()
    );
    Ok(())
}

/// Hashes the codes that migration 20261019120000 carried over as they
/// were, which SQL couldn't hash. Every hash is HASH_LENGTH bytes, and no
/// code ever was.
async fn hash_carried_over_codes(connection: &mut SqliteConnection) -> sqlx::Result<()> {
    let length = invite_code::HASH_LENGTH as i64;
    let codes = sqlx::query_scalar!(
        r#"SELECT code_hash AS "code!" FROM invites WHERE length(code_hash) != ?"#,
        length
    )
    .fetch_all(&mut *connection)
    .await?;
    for code in codes {
        let hash = invite_code::hash(&String::from_utf8_lossy(&code));
        sqlx::query!(
            "UPDATE invites SET code_hash = ? WHERE code_hash = ?",
            hash,
            code
        )
        .execute(&mut *connection)
        .await?;
    }
    Ok(())
}

async fn revoke_open_invites(
    executor: impl SqliteExecutor<'_>,
    account: &str,
    now: i64,
) -> sqlx::Result<u64> {
    let revoked = sqlx::query!(
        "UPDATE invites SET revoked_at = ?
             WHERE created_by = ? AND redeemed_at IS NULL AND revoked_at IS NULL
               AND expires_at > ?",
        now,
        account,
        now,
    )
    .execute(executor)
    .await?;
    Ok(revoked.rows_affected())
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

    pub(crate) async fn execute(db: &Db, sql: &str) -> sqlx::Result<()> {
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

    /// Applies the migrations up to `version` to a new database at `path`
    /// and runs `sql` there, as a concierge of that version would have.
    async fn written_by_version(path: &Path, version: i64, sql: &str) {
        let older = Migrator {
            migrations: MIGRATOR
                .iter()
                .filter(|migration| migration.version <= version)
                .cloned()
                .collect(),
            ..Migrator::DEFAULT
        };
        let options = SqliteConnectOptions::new()
            .filename(path)
            .create_if_missing(true)
            .foreign_keys(true);
        let pool = SqlitePool::connect_with(options).await.unwrap();
        older.run(&pool).await.unwrap();
        sqlx::raw_sql(sql).execute(&pool).await.unwrap();
        pool.close().await;
    }

    #[tokio::test]
    async fn dropping_the_invite_quota_keeps_members_and_whatever_refers_to_them() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("concierge.db");
        written_by_version(
            &path,
            20261016120000,
            "INSERT INTO members (account, email, joined_at, invite_quota, is_admin)
             VALUES ('alice', 'alice@example.org', 1, 5, TRUE);
             INSERT INTO members (account, invited_by, joined_at, invite_quota, removed_at)
             VALUES ('bob', 'alice', 2, 0, 3);
             INSERT INTO invites (code, created_by, created_at, expires_at) VALUES ('open', 'alice', 4, 5);
             INSERT INTO sessions (token_hash, account, created_at, expires_at) VALUES (x'00', 'alice', 6, 70);
             INSERT INTO oauth_tokens (token_hash, session_hash, client_id, created_at, expires_at)
             VALUES (x'01', x'00', 'gamja', 6, 70);
             INSERT INTO bots (account, owner, created_at) VALUES ('bender', 'alice', 8);",
        )
        .await;

        let db = Db::open(&path).await.unwrap();

        assert_eq!(
            db.session(b"\x00", 10)
                .await
                .unwrap()
                .unwrap()
                .member
                .account,
            "alice"
        );
        assert!(db.access_token(b"\x01", 10).await.unwrap().is_some());

        let alice = db.member("ALICE").await.unwrap().unwrap();
        assert_eq!(alice.email.as_deref(), Some("alice@example.org"));
        assert!(alice.is_admin);
        let bob = db.member("bob").await.unwrap().unwrap();
        assert_eq!(
            (bob.invited_by.as_deref(), bob.removed_at),
            (Some("alice"), Some(3))
        );
        assert!(db.invite("open").await.unwrap().is_some());
        assert_eq!(db.bots(Some("alice")).await.unwrap().len(), 1);
        let error = execute(
            &db,
            "INSERT INTO bots (account, owner, created_at) VALUES ('x', 'nobody', 0)",
        )
        .await
        .unwrap_err();
        assert!(error.to_string().contains("FOREIGN KEY"), "{error}");
    }

    #[tokio::test]
    async fn codes_kept_before_hashing_are_hashed_and_still_found() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("concierge.db");
        written_by_version(
            &path,
            20261018120000,
            "INSERT INTO members (account, joined_at) VALUES ('alice', 0);
             INSERT INTO invites (code, created_by, created_at, expires_at)
             VALUES ('q7Hx2mZp9vLcT4wRfBa-_w', 'alice', 1, 2), ('v0Ri8dEa2kWnL5yTcHq-_x', NULL, 3, 4);",
        )
        .await;

        let db = Db::open(&path).await.unwrap();

        let invite = db.invite("q7Hx2mZp9vLcT4wRfBa-_w").await.unwrap().unwrap();
        assert_eq!(invite.created_by.as_deref(), Some("alice"));
        assert!(db.invite("v0Ri8dEa2kWnL5yTcHq-_x").await.unwrap().is_some());
        let kept: Vec<Vec<u8>> = sqlx::query_scalar("SELECT code_hash FROM invites ORDER BY rowid")
            .fetch_all(&db.pool)
            .await
            .unwrap();
        let hashes = ["q7Hx2mZp9vLcT4wRfBa-_w", "v0Ri8dEa2kWnL5yTcHq-_x"].map(invite_code::hash);
        assert_eq!(kept, hashes);
    }

    #[tokio::test]
    async fn hashing_finishes_what_an_interrupted_open_started_and_then_changes_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("concierge.db");
        written_by_version(
            &path,
            20261019120000,
            "INSERT INTO invites (code_hash, created_at, expires_at)
             VALUES (CAST('q7Hx2mZp9vLcT4wRfBa-_w' AS BLOB), 1, 2);",
        )
        .await;
        let hashed = invite_code::hash("v0Ri8dEa2kWnL5yTcHq-_x");
        let pool = SqlitePool::connect(&format!("sqlite://{}", path.display()))
            .await
            .unwrap();
        sqlx::query("INSERT INTO invites (code_hash, created_at, expires_at) VALUES (?, 3, 4)")
            .bind(&hashed)
            .execute(&pool)
            .await
            .unwrap();
        pool.close().await;

        Db::open(&path).await.unwrap();
        let db = Db::open(&path).await.unwrap();

        let kept: Vec<Vec<u8>> = sqlx::query_scalar("SELECT code_hash FROM invites ORDER BY rowid")
            .fetch_all(&db.pool)
            .await
            .unwrap();
        assert_eq!(kept, [invite_code::hash("q7Hx2mZp9vLcT4wRfBa-_w"), hashed]);
    }

    #[tokio::test]
    async fn a_database_whose_references_dont_hold_doesnt_open() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("concierge.db");
        written_by_version(
            &path,
            20261016120000,
            "PRAGMA foreign_keys = OFF;
             INSERT INTO sessions (token_hash, account, created_at, expires_at) VALUES (x'00', 'ghost', 0, 1);",
        )
        .await;

        let error = Db::open(&path).await.err().unwrap();

        assert!(
            format!("{error:#}").contains("1 rows refer to rows that aren't there"),
            "{error:#}"
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
            "INSERT INTO invites (code_hash, created_by, created_at, expires_at) VALUES (x'00', 'nobody', 0, 1)",
        )
        .await
        .unwrap_err();
        assert!(error.to_string().contains("FOREIGN KEY"), "{error}");
    }

    #[tokio::test]
    async fn account_names_are_unique_whatever_their_case() {
        let (db, _dir) = temp_db().await;
        let insert =
            |name: &str| format!("INSERT INTO members (account, joined_at) VALUES ('{name}', 0)");
        execute(&db, &insert("Alice")).await.unwrap();
        let error = execute(&db, &insert("aLICE")).await.unwrap_err();
        assert!(error.to_string().contains("UNIQUE"), "{error}");
    }

    #[tokio::test]
    async fn a_removed_member_keeps_nothing_personal() {
        let (db, _dir) = temp_db().await;
        let error = execute(
            &db,
            "INSERT INTO members (account, email, joined_at, removed_at)
             VALUES ('alice', 'alice@example.org', 0, 1)",
        )
        .await
        .unwrap_err();
        assert!(error.to_string().contains("CHECK"), "{error}");
        execute(
            &db,
            "INSERT INTO members (account, joined_at, removed_at) VALUES ('alice', 0, 1)",
        )
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn removing_a_member_keeps_the_invites_that_shaped_the_tree() {
        let (db, _dir) = temp_db().await;
        execute(
            &db,
            "INSERT INTO members (account, email, invited_by, joined_at)
             VALUES ('root', NULL, NULL, 0),
                    ('alice', 'alice@example.org', 'root', 10),
                    ('bob', 'bob@example.org', 'alice', 20);
             UPDATE members SET email_verified_at = 11, news_opt_in = TRUE, is_admin = TRUE
             WHERE account = 'alice';",
        )
        .await
        .unwrap();
        for (code, created_by, created_at, expires_at) in [
            ("to-alice", "root", 5, 100),
            ("to-bob", "alice", 15, 100),
            ("stale", "alice", 15, 100),
            ("open", "alice", 150, 300),
        ] {
            add_invite(&db, code, Some(created_by), created_at, expires_at).await;
        }
        update_invite(&db, "to-alice", "redeemed_by = 'alice', redeemed_at = 10").await;
        update_invite(&db, "to-bob", "redeemed_by = 'bob', redeemed_at = 20").await;

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
            assert_eq!(invite.redeemed_by.as_deref(), redeemed_by, "{code}");
            assert_eq!(invite.revoked_at, revoked_at, "{code}");
        }
        let bob = db.member("bob").await.unwrap().unwrap();
        assert_eq!(bob.invited_by.as_deref(), Some("alice"));
        assert_eq!(bob.email.as_deref(), Some("bob@example.org"));
    }

    #[tokio::test]
    async fn revoking_a_members_open_invites_leaves_every_other_invite_alone() {
        let (db, _dir) = temp_db().await;
        execute(
            &db,
            "INSERT INTO members (account, joined_at)
             VALUES ('alice', 0), ('bob', 0), ('carol', 0);",
        )
        .await
        .unwrap();
        for (code, created_by, expires_at) in [
            ("open", "alice", 300),
            ("claimed", "alice", 300),
            ("redeemed", "alice", 300),
            ("expired", "alice", 100),
            ("revoked", "alice", 300),
            ("bobs", "bob", 300),
        ] {
            add_invite(&db, code, Some(created_by), 50, expires_at).await;
        }
        update_invite(&db, "claimed", "claimed_at = 190").await;
        update_invite(&db, "redeemed", "redeemed_by = 'carol', redeemed_at = 60").await;
        update_invite(&db, "revoked", "revoked_at = 70").await;

        let revoked = db.revoke_open_invites("alice", 200).await.unwrap();

        assert_eq!(revoked, 2);
        for (code, revoked_at) in [
            ("open", Some(200)),
            ("claimed", Some(200)),
            ("redeemed", None),
            ("expired", None),
            ("revoked", Some(70)),
            ("bobs", None),
        ] {
            let invite = db.invite(code).await.unwrap().unwrap();
            assert_eq!(invite.revoked_at, revoked_at, "{code}");
        }
    }

    #[tokio::test]
    async fn a_removed_members_email_and_mail_leave_the_database_files() {
        let (db, dir) = with_outbox().await;
        let secure_delete: i64 = sqlx::query_scalar("PRAGMA secure_delete")
            .fetch_one(&db.pool)
            .await
            .unwrap();
        assert_eq!(secure_delete, 1);

        let removal = db.remove_member("alice", 200).await.unwrap();

        assert!(removal.off_the_disk);
        assert!(!files_contain(&dir, b"alice@example.org"));
        assert!(files_contain(&dir, b"bob@example.org"));
        let due = db.due_messages(QUEUED + 60).await.unwrap();
        assert_eq!(recipients(&due), ["bob@example.org"]);
    }

    #[tokio::test]
    async fn a_removed_members_mail_goes_but_not_others_mail_to_the_same_address() {
        let (db, _dir) = with_outbox().await;
        execute(
            &db,
            "INSERT INTO members (account, email, joined_at)
             VALUES ('carol', 'alice@example.org', 30);",
        )
        .await
        .unwrap();
        for account in [Some("carol"), None] {
            let message = message(account, "alice@example.org", VERIFY_SUBJECT);
            db.queue_message(&message, QUEUED + 120).await.unwrap();
        }

        db.remove_member("alice", 200).await.unwrap();

        let due = db.due_messages(QUEUED + 120).await.unwrap();
        assert_eq!(
            recipients(&due),
            ["bob@example.org", "alice@example.org", "alice@example.org"]
        );
    }

    /// Whether any of the database's files in `dir` holds `text`.
    fn files_contain(dir: &tempfile::TempDir, text: &[u8]) -> bool {
        let on_disk: Vec<u8> = std::fs::read_dir(dir.path())
            .unwrap()
            .flat_map(|entry| std::fs::read(entry.unwrap().path()).unwrap())
            .collect();
        on_disk.windows(text.len()).any(|window| window == text)
    }

    #[tokio::test]
    async fn a_reader_holding_the_log_leaves_the_removal_for_the_vacuum() {
        let (db, dir) = temp_db().await;
        execute(
            &db,
            "INSERT INTO members (account, email, joined_at)
             VALUES ('alice', 'alice@example.org', 10);",
        )
        .await
        .unwrap();
        let reader = Db::open(&dir.path().join("concierge.db")).await.unwrap();
        let mut snapshot = reader.pool.begin().await.unwrap();
        sqlx::query("SELECT COUNT(*) FROM members")
            .execute(&mut *snapshot)
            .await
            .unwrap();

        let removal = db.remove_member("alice", 200).await.unwrap();

        assert!(!removal.off_the_disk);
        assert_eq!(
            db.member("alice").await.unwrap().unwrap().removed_at,
            Some(200)
        );
    }

    const CREATED: i64 = 1_000;

    /// Adds `code`'s invite the way the site does.
    async fn add_invite(
        db: &Db,
        code: &str,
        created_by: Option<&str>,
        created_at: i64,
        expires_at: i64,
    ) {
        let invite = NewInvite {
            code,
            created_by,
            created_at,
            expires_at,
            makes_admin: false,
        };
        db.insert_invite(&invite).await.unwrap();
    }

    /// Sets `assignments` on `code`'s invite, which no query of the site's
    /// does, to set up a test.
    async fn update_invite(db: &Db, code: &str, assignments: &str) {
        sqlx::query(&format!(
            "UPDATE invites SET {assignments} WHERE code_hash = ?"
        ))
        .bind(invite_code::hash(code))
        .execute(&db.pool)
        .await
        .unwrap();
    }

    /// alice invited someone with the code "open", which nobody used yet.
    async fn with_open_invite() -> (Db, tempfile::TempDir) {
        let (db, dir) = temp_db().await;
        execute(
            &db,
            "INSERT INTO members (account, joined_at) VALUES ('alice', 0)",
        )
        .await
        .unwrap();
        let expires_at = invite_rules::expires_at(CREATED);
        add_invite(&db, "open", Some("alice"), CREATED, expires_at).await;
        (db, dir)
    }

    fn newcomer(account: &str, joined_at: i64) -> NewMember<'_> {
        NewMember {
            account,
            email: Some("bob@example.org"),
            news_opt_in: true,
            invited_by: Some("alice"),
            joined_at,
            is_admin: false,
        }
    }

    #[tokio::test]
    async fn claiming_holds_an_open_invite_for_one_redemption() {
        let (db, _dir) = with_open_invite().await;
        let now = CREATED + 10;
        let claim = db.claim_invite("open", now).await.unwrap().unwrap();
        assert_eq!(
            claim,
            Claim {
                code_hash: invite_code::hash("open"),
                claimed_at: now,
                created_by: Some("alice".into()),
                makes_admin: false,
            }
        );
        assert_eq!(db.claim_invite("open", now).await.unwrap(), None);
        assert_eq!(db.claim_invite("unknown", now).await.unwrap(), None);
    }

    #[tokio::test]
    async fn a_released_claim_frees_the_invite() {
        let (db, _dir) = with_open_invite().await;
        let claim = db.claim_invite("open", CREATED).await.unwrap().unwrap();
        db.release_claim(&claim).await.unwrap();
        assert!(db.claim_invite("open", CREATED).await.unwrap().is_some());
    }

    #[tokio::test]
    async fn a_stale_claim_is_taken_over_and_can_neither_release_nor_redeem() {
        let (db, _dir) = with_open_invite().await;
        let stale = db.claim_invite("open", CREATED).await.unwrap().unwrap();
        let later = CREATED + invite_rules::CLAIM_TIMEOUT;
        let fresh = db.claim_invite("open", later).await.unwrap().unwrap();

        db.release_claim(&stale).await.unwrap();
        assert_eq!(db.claim_invite("open", later).await.unwrap(), None);
        let redeemed = db.redeem_invite(&stale, &newcomer("bob", later)).await;
        assert_eq!(redeemed.unwrap(), Redemption::Lost);
        assert_eq!(db.member("bob").await.unwrap(), None);

        let redeemed = db.redeem_invite(&fresh, &newcomer("carol", later)).await;
        assert_eq!(redeemed.unwrap(), Redemption::Redeemed);
    }

    #[tokio::test]
    async fn redeeming_adds_the_member_and_uses_the_invite_up() {
        let (db, _dir) = with_open_invite().await;
        let now = CREATED + 10;
        let claim = db.claim_invite("open", now).await.unwrap().unwrap();

        let redeemed = db.redeem_invite(&claim, &newcomer("Bob", now)).await;

        assert_eq!(redeemed.unwrap(), Redemption::Redeemed);
        let bob = db.member("bob").await.unwrap().unwrap();
        assert_eq!(bob.invited_by.as_deref(), Some("alice"));
        assert_eq!(bob.email.as_deref(), Some("bob@example.org"));
        assert!(bob.news_opt_in);
        let invite = db.invite("open").await.unwrap().unwrap();
        assert_eq!(invite.redeemed_by.as_deref(), Some("Bob"));
        assert_eq!(invite.status(now), invite_rules::Status::Redeemed);
        db.release_claim(&claim).await.unwrap();
        assert_eq!(db.claim_invite("open", now + 1_000).await.unwrap(), None);
    }

    #[tokio::test]
    async fn an_invite_revoked_mid_redemption_adds_nobody() {
        let (db, _dir) = with_open_invite().await;
        let claim = db.claim_invite("open", CREATED).await.unwrap().unwrap();
        update_invite(&db, "open", "revoked_at = 1").await;
        let redeemed = db.redeem_invite(&claim, &newcomer("bob", CREATED)).await;
        assert_eq!(redeemed.unwrap(), Redemption::Lost);
        assert_eq!(db.member("bob").await.unwrap(), None);
    }

    #[tokio::test]
    async fn a_taken_name_fails_the_redemption_and_leaves_the_invite_unused() {
        let (db, _dir) = with_open_invite().await;
        let claim = db.claim_invite("open", CREATED).await.unwrap().unwrap();
        assert!(
            db.redeem_invite(&claim, &newcomer("ALICE", CREATED))
                .await
                .is_err()
        );
        assert_eq!(db.invite("open").await.unwrap().unwrap().redeemed_at, None);
    }

    #[tokio::test]
    async fn lists_a_members_invites_and_the_people_they_brought() {
        let (db, _dir) = with_open_invite().await;
        let claim = db.claim_invite("open", CREATED).await.unwrap().unwrap();
        db.redeem_invite(&claim, &newcomer("bob", CREATED + 5))
            .await
            .unwrap();
        let newer = CREATED + 10;
        for code in ["newer", "made-in-the-same-second"] {
            add_invite(
                &db,
                code,
                Some("alice"),
                newer,
                invite_rules::expires_at(newer),
            )
            .await;
        }

        let invites = db.invites_created_by("alice").await.unwrap();
        let hashes: Vec<_> = invites.into_iter().map(|invite| invite.code_hash).collect();
        let newest_first = ["made-in-the-same-second", "newer", "open"].map(invite_code::hash);
        assert_eq!(hashes, newest_first);
        assert_eq!(
            db.invitees("alice").await.unwrap(),
            [Invitee {
                account: "bob".into(),
                joined_at: CREATED + 5,
                removed: false,
            }]
        );
        assert!(db.invitees("bob").await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn expired_revoked_and_redeemed_invites_cannot_be_claimed() {
        let (db, _dir) = with_open_invite().await;
        let expired = invite_rules::expires_at(CREATED);
        assert_eq!(db.claim_invite("open", expired).await.unwrap(), None);
        update_invite(&db, "open", "revoked_at = 1").await;
        assert_eq!(db.claim_invite("open", CREATED).await.unwrap(), None);
    }

    async fn with_sessions() -> (Db, tempfile::TempDir) {
        let (db, dir) = temp_db().await;
        execute(
            &db,
            "INSERT INTO members (account, joined_at) VALUES ('alice', 0), ('bob', 0);
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
    async fn ending_the_other_sessions_keeps_one_and_leaves_other_members_alone() {
        let (db, _dir) = with_sessions().await;
        execute(
            &db,
            "INSERT INTO oauth_tokens (token_hash, session_hash, client_id, created_at, expires_at)
             VALUES (x'f1', x'a1', 'gamja', 0, 100), (x'f2', x'a2', 'gamja', 0, 100)",
        )
        .await
        .unwrap();
        assert_eq!(db.end_other_sessions("alice", b"\xa2").await.unwrap(), 1);
        assert_eq!(db.session(b"\xa1", 0).await.unwrap(), None);
        assert!(db.session(b"\xa2", 0).await.unwrap().is_some());
        assert!(db.session(b"\xb1", 0).await.unwrap().is_some());
        assert_eq!(db.access_token(b"\xf1", 0).await.unwrap(), None);
        assert!(db.access_token(b"\xf2", 0).await.unwrap().is_some());
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
            "INSERT INTO members (account, email, email_verified_at, joined_at)
             VALUES ('alice', 'alice@example.org', 10, 0)",
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

    fn code<'a>(
        session_hash: &'a [u8],
        code_hash: &'a [u8],
        expires_at: i64,
    ) -> NewAuthorizationCode<'a> {
        NewAuthorizationCode {
            code_hash,
            session_hash,
            client_id: "gamja",
            redirect_uri: "https://irc.weblounge.org/",
            code_challenge: "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM",
            expires_at,
        }
    }

    fn token<'a>(
        session_hash: &'a [u8],
        token_hash: &'a [u8],
        expires_at: i64,
    ) -> NewAccessToken<'a> {
        NewAccessToken {
            token_hash,
            session_hash,
            client_id: "gamja",
            created_at: 0,
            expires_at,
        }
    }

    #[tokio::test]
    async fn an_authorization_code_can_be_taken_once() {
        let (db, _dir) = with_sessions().await;
        db.insert_authorization_code(&code(b"\xa1", b"\xc1", 60), 0)
            .await
            .unwrap();
        let taken = db.take_authorization_code(b"\xc1").await.unwrap().unwrap();
        assert_eq!(
            taken,
            AuthorizationCode {
                session_hash: b"\xa1".to_vec(),
                client_id: "gamja".into(),
                redirect_uri: "https://irc.weblounge.org/".into(),
                code_challenge: "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM".into(),
                expires_at: 60,
            }
        );
        assert_eq!(db.take_authorization_code(b"\xc1").await.unwrap(), None);
        assert_eq!(db.take_authorization_code(b"\xff").await.unwrap(), None);
    }

    #[tokio::test]
    async fn expired_codes_are_cleaned_up_when_another_is_issued() {
        let (db, _dir) = with_sessions().await;
        db.insert_authorization_code(&code(b"\xa1", b"\xc1", 60), 0)
            .await
            .unwrap();
        db.insert_authorization_code(&code(b"\xa1", b"\xc2", 120), 60)
            .await
            .unwrap();
        assert_eq!(db.take_authorization_code(b"\xc1").await.unwrap(), None);
        assert!(db.take_authorization_code(b"\xc2").await.unwrap().is_some());
    }

    #[tokio::test]
    async fn an_access_token_names_its_member_until_it_expires() {
        let (db, _dir) = with_sessions().await;
        db.insert_access_token(&token(b"\xa2", b"\xd1", 150))
            .await
            .unwrap();
        let live = db.access_token(b"\xd1", 149).await.unwrap();
        assert_eq!(
            live,
            Some(AccessToken {
                account: "alice".into(),
                client_id: "gamja".into(),
                expires_at: 150,
            })
        );
        assert_eq!(db.access_token(b"\xd1", 150).await.unwrap(), None);
        assert_eq!(db.access_token(b"\xff", 0).await.unwrap(), None);
    }

    #[tokio::test]
    async fn codes_and_tokens_end_with_their_session() {
        let (db, _dir) = with_sessions().await;
        db.insert_authorization_code(&code(b"\xa1", b"\xc1", 60), 0)
            .await
            .unwrap();
        db.insert_access_token(&token(b"\xa1", b"\xd1", 300))
            .await
            .unwrap();
        db.insert_access_token(&token(b"\xa2", b"\xd2", 300))
            .await
            .unwrap();
        assert_eq!(
            db.access_token(b"\xd1", 100).await.unwrap(),
            None,
            "a1 expired"
        );
        db.end_session(b"\xa1").await.unwrap();
        assert_eq!(db.take_authorization_code(b"\xc1").await.unwrap(), None);
        assert!(db.access_token(b"\xd2", 0).await.unwrap().is_some());
        db.end_sessions("alice").await.unwrap();
        assert_eq!(db.access_token(b"\xd2", 0).await.unwrap(), None);
    }

    #[tokio::test]
    async fn a_token_needs_an_existing_session() {
        let (db, _dir) = with_sessions().await;
        assert!(
            db.insert_access_token(&token(b"\xee", b"\xd1", 300))
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn a_removed_members_tokens_name_nobody() {
        let (db, _dir) = with_sessions().await;
        db.insert_access_token(&token(b"\xa2", b"\xd1", 300))
            .await
            .unwrap();
        execute(
            &db,
            "UPDATE members SET removed_at = 1 WHERE account = 'alice'",
        )
        .await
        .unwrap();
        assert_eq!(db.access_token(b"\xd1", 0).await.unwrap(), None);
    }

    /// luiz runs bender and dalek, alice runs Marvin; dalek was deleted.
    async fn with_bots() -> (Db, tempfile::TempDir) {
        let (db, dir) = temp_db().await;
        execute(
            &db,
            "INSERT INTO members (account, joined_at)
             VALUES ('luiz', 0), ('alice', 0)",
        )
        .await
        .unwrap();
        for (account, owner, note) in [
            ("bender", "luiz", Some("answers !ping")),
            ("dalek", "luiz", None),
            ("Marvin", "alice", Some("posts the weather")),
        ] {
            db.insert_bot(&NewBot {
                account,
                owner,
                note,
                created_at: 100,
            })
            .await
            .unwrap();
        }
        assert!(db.remove_bot("DALEK", 200).await.unwrap());
        (db, dir)
    }

    #[tokio::test]
    async fn a_bot_is_found_by_its_name_in_any_case() {
        let (db, _dir) = with_bots().await;
        assert_eq!(
            db.bot("BENDER").await.unwrap(),
            Some(Bot {
                account: "bender".into(),
                owner: "luiz".into(),
                note: Some("answers !ping".into()),
                created_at: 100,
                removed_at: None,
            })
        );
        assert_eq!(
            db.bot("dalek").await.unwrap().unwrap().removed_at,
            Some(200)
        );
        assert_eq!(db.bot("luiz").await.unwrap(), None);
    }

    #[tokio::test]
    async fn lists_the_live_bots_of_one_owner_or_of_everyone() {
        let (db, _dir) = with_bots().await;
        let names =
            |bots: Vec<Bot>| -> Vec<String> { bots.into_iter().map(|bot| bot.account).collect() };
        assert_eq!(names(db.bots(None).await.unwrap()), ["bender", "Marvin"]);
        assert_eq!(names(db.bots(Some("LUIZ")).await.unwrap()), ["bender"]);
        assert!(db.bots(Some("nobody")).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn lists_every_bot_with_whether_it_or_its_owner_is_gone() {
        let (db, _dir) = with_bots().await;
        execute(
            &db,
            "UPDATE members SET removed_at = 300 WHERE account = 'alice'",
        )
        .await
        .unwrap();
        let account = |account: &str, owner: &str, removed, owner_removed| BotAccount {
            account: account.into(),
            owner: owner.into(),
            removed,
            owner_removed,
        };
        assert_eq!(
            db.bot_accounts().await.unwrap(),
            [
                account("bender", "luiz", false, false),
                account("dalek", "luiz", true, false),
                account("Marvin", "alice", false, true),
            ]
        );
    }

    #[tokio::test]
    async fn a_bot_is_removed_once_and_needs_an_owner_and_a_free_name() {
        let (db, _dir) = with_bots().await;
        assert!(!db.remove_bot("dalek", 300).await.unwrap());
        assert!(!db.remove_bot("nobody", 300).await.unwrap());
        assert_eq!(
            db.bot("dalek").await.unwrap().unwrap().removed_at,
            Some(200)
        );
        let bot = |account, owner| NewBot {
            account,
            owner,
            note: None,
            created_at: 300,
        };
        let error = db.insert_bot(&bot("hal", "nobody")).await.unwrap_err();
        assert!(error.to_string().contains("FOREIGN KEY"), "{error}");
        let error = db.insert_bot(&bot("Dalek", "alice")).await.unwrap_err();
        assert!(error.to_string().contains("UNIQUE"), "{error}");
        db.forget_bot("DALEK").await.unwrap();
        assert!(!db.name_held("dalek").await.unwrap());
        db.insert_bot(&bot("Dalek", "alice")).await.unwrap();
    }

    #[tokio::test]
    async fn members_and_bots_hold_their_names_in_any_case_even_once_gone() {
        let (db, _dir) = with_bots().await;
        execute(
            &db,
            "INSERT INTO members (account, joined_at, removed_at)
             VALUES ('Gone', 0, 1)",
        )
        .await
        .unwrap();
        for name in ["luiz", "ALICE", "gone", "Bender", "marvin", "dalek"] {
            assert!(db.name_held(name).await.unwrap(), "{name}");
        }
        for name in ["bob", "bend", "luiz_"] {
            assert!(!db.name_held(name).await.unwrap(), "{name}");
        }
    }

    #[tokio::test]
    async fn a_removed_members_session_signs_nobody_in() {
        let (db, _dir) = with_sessions().await;
        execute(
            &db,
            "UPDATE members SET removed_at = 1 WHERE account = 'alice'",
        )
        .await
        .unwrap();
        assert_eq!(db.session(b"\xa2", 0).await.unwrap(), None);
    }

    pub(crate) const QUEUED: i64 = 1_760_000_000;
    pub(crate) const VERIFY_SUBJECT: &str = "Verify your email for weblounge";

    fn message(account: Option<&str>, to: &str, subject: &str) -> outbox::Message {
        outbox::Message {
            account: account.map(Into::into),
            to: crate::email::Email::parse(to).unwrap(),
            subject: outbox::Subject::parse(subject).unwrap(),
            body: format!(
                "Hi {},\n\n\
                 Open https://members.weblounge.org/verify#q7Hx2mZp9vLcT4wRfBa to verify\n\
                 the address you gave weblounge.org. Zoë says hello.\n",
                account.unwrap_or("there"),
            ),
        }
    }

    /// Members alice and bob, and alice's verification email, then bob's a
    /// minute later.
    pub(crate) async fn with_outbox() -> (Db, tempfile::TempDir) {
        let (db, dir) = temp_db().await;
        execute(
            &db,
            "INSERT INTO members (account, email, joined_at)
             VALUES ('alice', 'alice@example.org', 10),
                    ('bob', 'bob@example.org', 20);",
        )
        .await
        .unwrap();
        for (account, queued_at) in [("alice", QUEUED), ("bob", QUEUED + 60)] {
            let to = format!("{account}@example.org");
            let message = message(Some(account), &to, VERIFY_SUBJECT);
            db.queue_message(&message, queued_at).await.unwrap();
        }
        (db, dir)
    }

    fn recipients(messages: &[QueuedMessage]) -> Vec<&str> {
        messages
            .iter()
            .map(|message| message.recipient.as_str())
            .collect()
    }

    #[tokio::test]
    async fn queued_messages_are_due_at_once_oldest_first() {
        let (db, _dir) = with_outbox().await;
        let due = db.due_messages(QUEUED + 60).await.unwrap();
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
            recipients(&db.due_messages(QUEUED).await.unwrap()),
            ["alice@example.org"]
        );
    }

    #[tokio::test]
    async fn nothing_is_due_before_anything_is_queued() {
        let (db, _dir) = temp_db().await;
        assert!(db.due_messages(QUEUED).await.unwrap().is_empty());
        let (db, _dir) = with_outbox().await;
        assert!(db.due_messages(QUEUED - 1).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn a_failed_attempt_puts_the_message_off() {
        let (db, _dir) = with_outbox().await;
        let alice = db.due_messages(QUEUED).await.unwrap().remove(0);

        db.record_failed_attempt(&alice, QUEUED).await.unwrap();

        let retry_at = outbox::next_attempt_at(1, QUEUED);
        assert!(db.due_messages(retry_at - 1).await.unwrap().is_empty());
        let due = db.due_messages(retry_at).await.unwrap();
        assert_eq!(recipients(&due), ["alice@example.org", "bob@example.org"]);
        assert_eq!(due[0].attempts, 1);
        db.record_failed_attempt(&due[0], retry_at).await.unwrap();
        let again_at = outbox::next_attempt_at(2, retry_at);
        let due = db.due_messages(again_at - 1).await.unwrap();
        assert_eq!(recipients(&due), ["bob@example.org"]);
        let due = db.due_messages(again_at).await.unwrap();
        assert_eq!(
            (due[0].recipient.as_str(), due[0].attempts),
            ("alice@example.org", 2)
        );
    }

    #[tokio::test]
    async fn messages_given_up_on_are_never_due_and_get_deleted() {
        let (db, _dir) = with_outbox().await;
        let alices_end = QUEUED + outbox::GIVE_UP_AFTER + 1;
        assert_eq!(
            recipients(&db.due_messages(alices_end).await.unwrap()),
            ["bob@example.org"]
        );

        let deletion = db.delete_given_up_messages(alices_end).await.unwrap();

        assert_eq!(
            deletion,
            Deletion {
                messages: 1,
                off_the_disk: true,
            }
        );
        let due = db.due_messages(QUEUED + 60).await.unwrap();
        assert_eq!(recipients(&due), ["bob@example.org"]);
        let deletion = db.delete_given_up_messages(alices_end).await.unwrap();
        assert_eq!(deletion.messages, 0);
    }

    #[tokio::test]
    async fn a_sent_message_leaves_the_database_files() {
        let (db, dir) = with_outbox().await;
        let alice = db.due_messages(QUEUED).await.unwrap().remove(0);

        let deletion = db.delete_message(alice.id).await.unwrap();

        assert_eq!(
            deletion,
            Deletion {
                messages: 1,
                off_the_disk: true,
            }
        );
        assert!(!files_contain(&dir, b"Hi alice,"));
        assert!(files_contain(&dir, b"Hi bob,"));
        assert_eq!(db.delete_message(alice.id).await.unwrap().messages, 0);
    }
}
