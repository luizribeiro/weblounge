mod uploads;

use std::path::Path;

use anyhow::Context;
use common::sqlite;
use secret::SecretString;
use sqlx::{SqliteConnection, SqliteExecutor, SqlitePool, migrate::Migrator};

use crate::{
    email::Email,
    invite_code, invite_rules, link_code, link_mail,
    name::{BotName, MemberName},
    oauth::CodeChallenge,
    reset_rules::{self, Lookup},
    secret_hash::{
        AccessTokenHash, AuthorizationCodeHash, InviteCodeHash, ResetCodeHash, SessionHash,
        VerificationCodeHash,
    },
    tree::Branch,
    verification_rules,
};

static MIGRATOR: Migrator = sqlx::migrate!();

pub use common::now;
pub use uploads::{NewUpload, Upload};

pub struct NewMember<'a> {
    /// Ergo's canonical spelling of the account name.
    pub account: &'a MemberName,
    pub email: &'a Email,
    pub news_opt_in: bool,
    /// `None` for a root of the invite tree.
    pub invited_by: Option<&'a MemberName>,
    pub joined_at: i64,
    pub is_admin: bool,
}

/// A member, or the placeholder a removed member leaves in the invite tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Member {
    pub account: MemberName,
    pub email: Option<Email>,
    pub email_verified_at: Option<i64>,
    pub news_opt_in: bool,
    pub invited_by: Option<MemberName>,
    pub joined_at: i64,
    pub is_admin: bool,
    pub removed_at: Option<i64>,
    /// When the first email to verify their address went out, which
    /// starts their grace period.
    pub verification_first_sent_at: Option<i64>,
    pub verification_reminded_at: Option<i64>,
}

/// A member's name, and whether only their placeholder is left.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemberAccount {
    pub account: MemberName,
    pub removed: bool,
}

pub struct NewSession<'a> {
    pub token_hash: &'a SessionHash,
    pub account: &'a MemberName,
    pub created_at: i64,
    pub expires_at: i64,
}

pub struct NewAuthorizationCode<'a> {
    pub code_hash: &'a AuthorizationCodeHash,
    /// The session that approved it, whose member it signs in.
    pub session_hash: &'a SessionHash,
    pub client_id: &'a str,
    pub redirect_uri: &'a str,
    pub code_challenge: &'a CodeChallenge,
    pub expires_at: i64,
}

/// An authorization code, taken out of the database by its one use.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthorizationCode {
    pub session_hash: SessionHash,
    pub client_id: String,
    pub redirect_uri: String,
    pub code_challenge: CodeChallenge,
    pub expires_at: i64,
}

pub struct NewAccessToken<'a> {
    pub token_hash: &'a AccessTokenHash,
    pub session_hash: &'a SessionHash,
    pub client_id: &'a str,
    pub created_at: i64,
    pub expires_at: i64,
}

/// A live access token, of a member who hasn't been removed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccessToken {
    pub account: MemberName,
    pub client_id: String,
    pub expires_at: i64,
}

pub struct NewInvite<'a> {
    /// The code itself, which only its hash is kept of.
    pub code: &'a str,
    /// `None` for a root invite.
    pub created_by: Option<&'a MemberName>,
    pub created_at: i64,
    pub expires_at: i64,
    /// Whether whoever redeems it becomes an admin.
    pub makes_admin: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Invite {
    pub code_hash: InviteCodeHash,
    pub created_by: Option<MemberName>,
    pub created_at: i64,
    pub expires_at: i64,
    pub claimed_at: Option<i64>,
    pub redeemed_by: Option<MemberName>,
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
    pub code_hash: InviteCodeHash,
    pub claimed_at: i64,
    pub created_by: Option<MemberName>,
    pub makes_admin: bool,
}

/// Someone a member invited.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Invitee {
    pub account: MemberName,
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

/// A newcomer's welcome so far, from [`Db::arrivals`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Arrival {
    pub account: MemberName,
    /// The member who invited them: root invites record no arrival, and
    /// removing a member takes their invitees' arrivals with them.
    pub inviter: MemberName,
    pub joined_at: i64,
    /// When they were first seen with a session attached.
    pub arrived_at: Option<i64>,
    /// When their welcome went out in #lounge.
    pub announced_at: Option<i64>,
    pub inviter_told_at: Option<i64>,
    pub nudged_at: Option<i64>,
    pub oriented_at: Option<i64>,
}

/// A step of a newcomer's welcome, which [`Db::record`] notes the time of.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Milestone<'a> {
    Arrived,
    /// The welcome went out, as the message with this id, if the server
    /// gave one.
    Announced(Option<&'a str>),
    InviterTold,
    Oriented,
    Nudged,
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
    pub account: &'a BotName,
    pub owner: &'a MemberName,
    pub note: Option<&'a str>,
    pub created_at: i64,
}

/// A bot, or the row a deleted bot leaves to hold its name until it's
/// free again.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bot {
    pub account: BotName,
    pub owner: MemberName,
    pub note: Option<String>,
    pub created_at: i64,
    pub removed_at: Option<i64>,
}

/// What became of inserting a bot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BotInsertion {
    Inserted,
    /// Another bot holds the name.
    NameTaken,
    /// Its owner has as many live bots as they may.
    TooMany,
}

/// A bot's name and owner, and which of the two is gone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BotAccount {
    pub account: BotName,
    pub owner: MemberName,
    pub removed: bool,
    pub owner_removed: bool,
}

/// A code just issued to verify a member's email, to mail to them once.
#[derive(Debug)]
pub struct IssuedCode {
    pub code: SecretString,
    /// Ergo's canonical spelling of the member's account name.
    pub account: MemberName,
    /// The address it verifies, and goes to.
    pub email: Email,
    pub expires_at: i64,
    /// When the member's grace period ends.
    pub deadline: i64,
}

impl link_mail::Issued for IssuedCode {
    fn expires_at(&self) -> i64 {
        self.expires_at
    }
}

/// A code that verifies a member's email, as the database keeps it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerificationCode {
    pub account: MemberName,
    /// The address it was sent to, the only one it verifies.
    pub email: Email,
    pub created_at: i64,
    pub expires_at: i64,
    pub replaced_at: Option<i64>,
}

impl VerificationCode {
    pub fn status(&self, now: i64) -> verification_rules::Status {
        let code = verification_rules::Code {
            expires_at: self.expires_at,
            replaced_at: self.replaced_at,
        };
        code.status(now)
    }
}

/// What came of [`Db::verify_email`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verification {
    /// The member's email is verified now.
    Verified {
        account: MemberName,
    },
    /// No code is on file by that: mistyped, used already, or long expired.
    Unknown,
    Expired,
    /// A newer email went out, whose link is the one that works.
    Replaced,
    /// The member changed their address since, or was removed.
    AddressChanged,
    /// Another member verified the same address first.
    AddressTaken,
}

/// A current member with a confirmed email, which a reset link may go to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Recoverable {
    /// Ergo's canonical spelling of the member's account name.
    pub account: MemberName,
    pub email: Email,
}

/// A code just issued to reset a member's password, to mail to them once.
pub struct IssuedReset {
    pub code: SecretString,
    pub member: Recoverable,
    pub expires_at: i64,
}

impl link_mail::Issued for IssuedReset {
    fn expires_at(&self) -> i64 {
        self.expires_at
    }
}

/// A reset code taken out of the database by its one use, which
/// [`Db::restore_reset_code`] puts back if the reset fails.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TakenReset {
    code_hash: ResetCodeHash,
    /// Ergo's canonical spelling of the member's account name.
    pub account: MemberName,
    /// The confirmed address it went to.
    pub email: Email,
    created_at: i64,
    expires_at: i64,
}

/// Concierge's SQLite database, migrated to the schema this build knows.
#[derive(Clone)]
pub struct Db {
    pool: SqlitePool,
}

impl Db {
    /// Opens the database at `path`, which [`Db::migrate`] must have
    /// brought up to this build's schema.
    pub async fn open(path: &Path) -> anyhow::Result<Self> {
        let pool = sqlite::connect(path, false).await?;
        sqlite::ensure_migrated(&pool, &MIGRATOR, path).await?;
        Ok(Self { pool })
    }

    /// Opens the database at `path`, creating it if needed, and applies any
    /// migrations it lacks. SQLite doesn't keep two processes from
    /// migrating at once, so only `concierge migrate` calls this, before
    /// the services start.
    pub async fn migrate(path: &Path) -> anyhow::Result<Self> {
        let pool = sqlite::connect(path, true).await?;
        migrate(&pool)
            .await
            .with_context(|| format!("can't migrate the database {}", path.display()))?;
        Ok(Self { pool })
    }

    /// The newest migration applied, if any.
    pub async fn migration_version(&self) -> sqlx::Result<Option<i64>> {
        sqlite::migration_version(&self.pool).await
    }

    pub async fn insert_member(&self, member: &NewMember<'_>) -> sqlx::Result<()> {
        insert_member(&self.pool, member).await
    }

    /// The member whose account is `account`, in any case.
    pub async fn member(&self, account: &str) -> sqlx::Result<Option<Member>> {
        sqlx::query_as!(
            Member,
            r#"SELECT account AS "account!: MemberName", email AS "email: Email",
                      email_verified_at, news_opt_in,
                      invited_by AS "invited_by: MemberName", joined_at, is_admin, removed_at,
                      verification_first_sent_at, verification_reminded_at
               FROM members WHERE account = ?"#,
            account
        )
        .fetch_optional(&self.pool)
        .await
    }

    /// Sets a member's email and news consent. A changed address is no
    /// longer verified, and codes sent to the old one verify nothing.
    pub async fn update_settings(
        &self,
        account: &MemberName,
        email: &Email,
        news_opt_in: bool,
    ) -> sqlx::Result<()> {
        let mut transaction = self.pool.begin().await?;
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
        .execute(&mut *transaction)
        .await?;
        sqlx::query!(
            "DELETE FROM verification_codes WHERE account = ? AND email IS NOT ?",
            account,
            email,
        )
        .execute(&mut *transaction)
        .await?;
        sqlx::query!(
            "DELETE FROM password_reset_codes WHERE account = ? AND email IS NOT ?",
            account,
            email,
        )
        .execute(&mut *transaction)
        .await?;
        transaction.commit().await
    }

    /// Whether a member other than `account` verified `email`, in any
    /// case, as the index that keeps verified addresses unique compares
    /// them.
    pub async fn email_taken(&self, email: &Email, account: &MemberName) -> sqlx::Result<bool> {
        sqlx::query_scalar!(
            r#"SELECT EXISTS (
                   SELECT 1 FROM members
                   WHERE lower(email) = lower(?) AND email_verified_at IS NOT NULL
                     AND account != ?
               ) AS "taken!: bool""#,
            email,
            account,
        )
        .fetch_one(&self.pool)
        .await
    }

    /// Issues a code that verifies `account`'s current email, to be mailed
    /// at `now`, replacing any code they had; the first one starts their
    /// grace period. Cleans up everyone's expired codes while at it.
    /// Answers `None` unless `account` is a current, unverified member.
    pub async fn issue_verification_code(
        &self,
        account: &MemberName,
        now: i64,
    ) -> sqlx::Result<Option<IssuedCode>> {
        let mut transaction = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let member = sqlx::query!(
            r#"UPDATE members
               SET verification_first_sent_at = COALESCE(verification_first_sent_at, ?)
               WHERE account = ? AND removed_at IS NULL AND email_verified_at IS NULL
               RETURNING account AS "account!: MemberName", email AS "email!: Email",
                         verification_first_sent_at AS "first_sent_at!""#,
            now,
            account,
        )
        .fetch_optional(&mut *transaction)
        .await?;
        let Some(member) = member else {
            return Ok(None);
        };
        sqlx::query!("DELETE FROM verification_codes WHERE expires_at <= ?", now)
            .execute(&mut *transaction)
            .await?;
        sqlx::query!(
            "UPDATE verification_codes SET replaced_at = ? WHERE account = ? AND replaced_at IS NULL",
            now,
            member.account,
        )
        .execute(&mut *transaction)
        .await?;
        let code = link_code::generate(link_code::MAILED_LENGTH);
        let code_hash = VerificationCodeHash::of(code.expose());
        let expires_at = verification_rules::expires_at(now);
        sqlx::query!(
            "INSERT INTO verification_codes (code_hash, account, email, created_at, expires_at)
             VALUES (?, ?, ?, ?, ?)",
            code_hash,
            member.account,
            member.email,
            now,
            expires_at,
        )
        .execute(&mut *transaction)
        .await?;
        transaction.commit().await?;
        Ok(Some(IssuedCode {
            code,
            account: member.account,
            email: member.email,
            expires_at,
            deadline: verification_rules::deadline(member.first_sent_at),
        }))
    }

    /// Verifies the email that `code` was sent to, if it's still its
    /// member's, and uses up the member's codes; all or nothing, so racing
    /// requests can't both use one code.
    pub async fn verify_email(&self, code: &str, now: i64) -> sqlx::Result<Verification> {
        let mut transaction = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let code_hash = VerificationCodeHash::of(code);
        let Some(record) = verification_code(&mut *transaction, &code_hash).await? else {
            return Ok(Verification::Unknown);
        };
        match record.status(now) {
            verification_rules::Status::Live => {}
            verification_rules::Status::Expired => return Ok(Verification::Expired),
            verification_rules::Status::Replaced => return Ok(Verification::Replaced),
        }
        let verified = sqlx::query!(
            "UPDATE members SET email_verified_at = ?
             WHERE account = ? AND email = ? AND removed_at IS NULL",
            now,
            record.account,
            record.email,
        )
        .execute(&mut *transaction)
        .await;
        let verified = match verified {
            Err(sqlx::Error::Database(error)) if error.is_unique_violation() => {
                return Ok(Verification::AddressTaken);
            }
            verified => verified?,
        };
        if verified.rows_affected() == 0 {
            return Ok(Verification::AddressChanged);
        }
        delete_verification_codes(&mut *transaction, &record.account).await?;
        transaction.commit().await?;
        Ok(Verification::Verified {
            account: record.account,
        })
    }

    /// The verification code `code`, while it's on file.
    pub async fn verification_code(&self, code: &str) -> sqlx::Result<Option<VerificationCode>> {
        verification_code(&self.pool, &VerificationCodeHash::of(code)).await
    }

    /// How far each current member got with verifying their email.
    pub async fn verification_progress(
        &self,
    ) -> sqlx::Result<Vec<(MemberName, verification_rules::Progress)>> {
        let rows = sqlx::query!(
            r#"SELECT account AS "account!: MemberName",
                      email_verified_at IS NOT NULL AS "verified!: bool",
                      verification_first_sent_at AS first_sent_at,
                      verification_reminded_at AS reminded_at,
                      (SELECT max(created_at) FROM verification_codes AS codes
                       WHERE codes.account = members.account AND replaced_at IS NULL)
                        AS "last_sent_at: i64"
               FROM members WHERE removed_at IS NULL ORDER BY account"#
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|row| {
                let progress = verification_rules::Progress {
                    verified: row.verified,
                    first_sent_at: row.first_sent_at,
                    reminded_at: row.reminded_at,
                    last_sent_at: row.last_sent_at,
                };
                (row.account, progress)
            })
            .collect())
    }

    /// Notes that `account`'s one reminder went out at `now`.
    pub async fn record_reminder(&self, account: &MemberName, now: i64) -> sqlx::Result<()> {
        sqlx::query!(
            "UPDATE members SET verification_reminded_at = ? WHERE account = ?",
            now,
            account,
        )
        .execute(&self.pool)
        .await
        .map(drop)
    }

    /// The current member with a confirmed email that `lookup` names, by
    /// their nick or that address, in any case.
    pub async fn recoverable_member(&self, lookup: &Lookup) -> sqlx::Result<Option<Recoverable>> {
        match lookup {
            Lookup::Nick(nick) => sqlx::query_as!(
                Recoverable,
                r#"SELECT account AS "account!: MemberName", email AS "email!: Email" FROM members
                       WHERE account = ? AND removed_at IS NULL AND email_verified_at IS NOT NULL"#,
                nick
            )
            .fetch_optional(&self.pool)
            .await,
            Lookup::Email(email) => sqlx::query_as!(
                Recoverable,
                r#"SELECT account AS "account!: MemberName", email AS "email!: Email" FROM members
                       WHERE lower(email) = lower(?) AND removed_at IS NULL
                         AND email_verified_at IS NOT NULL"#,
                email
            )
            .fetch_optional(&self.pool)
            .await,
        }
    }

    /// Issues a code that resets `account`'s password, to be mailed at
    /// `now` to their confirmed email, replacing any code they had, and
    /// cleans up everyone's expired codes while at it. Answers `None`
    /// unless `account` is a current member with a confirmed email.
    pub async fn issue_reset_code(
        &self,
        account: &MemberName,
        now: i64,
    ) -> sqlx::Result<Option<IssuedReset>> {
        let mut transaction = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let member = sqlx::query_as!(
            Recoverable,
            r#"SELECT account AS "account!: MemberName", email AS "email!: Email" FROM members
               WHERE account = ? AND removed_at IS NULL AND email_verified_at IS NOT NULL"#,
            account
        )
        .fetch_optional(&mut *transaction)
        .await?;
        let Some(member) = member else {
            return Ok(None);
        };
        sqlx::query!(
            "DELETE FROM password_reset_codes WHERE expires_at <= ? OR account = ?",
            now,
            member.account,
        )
        .execute(&mut *transaction)
        .await?;
        let code = link_code::generate(link_code::MAILED_LENGTH);
        let code_hash = ResetCodeHash::of(code.expose());
        let expires_at = reset_rules::expires_at(now);
        sqlx::query!(
            "INSERT INTO password_reset_codes (code_hash, account, email, created_at, expires_at)
             VALUES (?, ?, ?, ?, ?)",
            code_hash,
            member.account,
            member.email,
            now,
            expires_at,
        )
        .execute(&mut *transaction)
        .await?;
        transaction.commit().await?;
        Ok(Some(IssuedReset {
            code,
            member,
            expires_at,
        }))
    }

    /// Takes `code` out of the database, if it's a reset code that works at
    /// `now`: unexpired, and sent to the address that's still its member's
    /// confirmed one. A code that doesn't work goes too.
    pub async fn take_reset_code(&self, code: &str, now: i64) -> sqlx::Result<Option<TakenReset>> {
        let mut transaction = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let code_hash = ResetCodeHash::of(code);
        let taken = sqlx::query!(
            r#"DELETE FROM password_reset_codes WHERE code_hash = ?
               RETURNING account AS "account: MemberName", email AS "email: Email",
                         created_at, expires_at"#,
            code_hash,
        )
        .fetch_optional(&mut *transaction)
        .await?;
        let Some(taken) = taken else {
            return Ok(None);
        };
        let still_theirs = sqlx::query_scalar!(
            r#"SELECT EXISTS (
                   SELECT 1 FROM members
                   WHERE account = ? AND email = ? AND email_verified_at IS NOT NULL
                     AND removed_at IS NULL
               ) AS "still_theirs!: bool""#,
            taken.account,
            taken.email,
        )
        .fetch_one(&mut *transaction)
        .await?;
        transaction.commit().await?;
        Ok(
            (still_theirs && now < taken.expires_at).then_some(TakenReset {
                code_hash,
                account: taken.account,
                email: taken.email,
                created_at: taken.created_at,
                expires_at: taken.expires_at,
            }),
        )
    }

    /// Puts back a code [`Db::take_reset_code`] took, unless a newer one
    /// replaced it meanwhile.
    pub async fn restore_reset_code(&self, taken: &TakenReset) -> sqlx::Result<()> {
        sqlx::query!(
            "INSERT OR IGNORE INTO password_reset_codes
                 (code_hash, account, email, created_at, expires_at)
             VALUES (?, ?, ?, ?, ?)",
            taken.code_hash,
            taken.account,
            taken.email,
            taken.created_at,
            taken.expires_at,
        )
        .execute(&self.pool)
        .await
        .map(drop)
    }

    /// Every member's account, removed ones included.
    pub async fn member_accounts(&self) -> sqlx::Result<Vec<MemberAccount>> {
        sqlx::query_as!(
            MemberAccount,
            r#"SELECT account AS "account!: MemberName",
                      removed_at IS NOT NULL AS "removed!: bool"
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
    pub async fn session(
        &self,
        token_hash: &SessionHash,
        now: i64,
    ) -> sqlx::Result<Option<Session>> {
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

    pub async fn renew_session(
        &self,
        token_hash: &SessionHash,
        expires_at: i64,
    ) -> sqlx::Result<()> {
        sqlx::query!(
            "UPDATE sessions SET expires_at = ? WHERE token_hash = ?",
            expires_at,
            token_hash,
        )
        .execute(&self.pool)
        .await
        .map(drop)
    }

    pub async fn end_session(&self, token_hash: &SessionHash) -> sqlx::Result<()> {
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
    pub async fn end_sessions(&self, account: &MemberName) -> sqlx::Result<u64> {
        delete_sessions(&self.pool, account).await
    }

    /// Ends every session of `account` but the one whose token hashes to
    /// `kept`, answering how many it ended. Their OAuth codes and tokens go
    /// with them.
    pub async fn end_other_sessions(
        &self,
        account: &MemberName,
        kept: &SessionHash,
    ) -> sqlx::Result<u64> {
        let deleted = sqlx::query!(
            "DELETE FROM sessions WHERE account = ? AND token_hash != ?",
            account,
            kept,
        )
        .execute(&self.pool)
        .await?;
        Ok(deleted.rows_affected())
    }

    /// Clears everything personal about a member, their verification and
    /// reset codes and their arrival included, so a newcomer not welcomed
    /// yet never is, ends their sessions and revokes their open invites.
    /// Their invitees' arrivals go too, so the greeter never names them.
    /// Their row stays as their place in the invite tree, so the people
    /// they invited stay attached. Mail waiting for them is in the
    /// outbox's own database.
    pub async fn remove_member(&self, account: &MemberName, now: i64) -> sqlx::Result<Removal> {
        let mut transaction = self.pool.begin().await?;
        sqlx::query!(
            "UPDATE members
             SET email = NULL, email_verified_at = NULL, news_opt_in = FALSE,
                 is_admin = FALSE, removed_at = ?,
                 verification_first_sent_at = NULL, verification_reminded_at = NULL
             WHERE account = ?",
            now,
            account,
        )
        .execute(&mut *transaction)
        .await?;
        delete_verification_codes(&mut *transaction, account).await?;
        sqlx::query!(
            "DELETE FROM password_reset_codes WHERE account = ?",
            account
        )
        .execute(&mut *transaction)
        .await?;
        sqlx::query!(
            "DELETE FROM arrivals WHERE account = ?1
                 OR account IN (SELECT account FROM members WHERE invited_by = ?1)",
            account
        )
        .execute(&mut *transaction)
        .await?;
        let sessions_ended = delete_sessions(&mut *transaction, account).await?;
        let invites_revoked = revoke_open_invites(&mut *transaction, account, now).await?;
        transaction.commit().await?;
        Ok(Removal {
            sessions_ended,
            invites_revoked,
            off_the_disk: sqlite::empty_write_ahead_log(&self.pool).await,
        })
    }

    /// Revokes the invites `account` made that are still open, or being
    /// redeemed right now; answers how many.
    pub async fn revoke_open_invites(&self, account: &MemberName, now: i64) -> sqlx::Result<u64> {
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
            r#"SELECT code_hash AS "code_hash!: InviteCodeHash", created_by AS "created_by: MemberName",
                      created_at, expires_at, claimed_at,
                      redeemed_by AS "redeemed_by: MemberName", redeemed_at, revoked_at, makes_admin
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
        let code_hash = InviteCodeHash::of(code);
        sqlx::query_as!(
            Invite,
            r#"SELECT code_hash AS "code_hash!: InviteCodeHash", created_by AS "created_by: MemberName",
                      created_at, expires_at, claimed_at,
                      redeemed_by AS "redeemed_by: MemberName", redeemed_at, revoked_at, makes_admin
               FROM invites WHERE code_hash = ?"#,
            code_hash
        )
        .fetch_optional(&self.pool)
        .await
    }

    /// The invites `account` created, newest first.
    pub async fn invites_created_by(&self, account: &MemberName) -> sqlx::Result<Vec<Invite>> {
        sqlx::query_as!(
            Invite,
            r#"SELECT code_hash AS "code_hash!: InviteCodeHash", created_by AS "created_by: MemberName",
                      created_at, expires_at, claimed_at,
                      redeemed_by AS "redeemed_by: MemberName", redeemed_at, revoked_at, makes_admin
               FROM invites WHERE created_by = ? ORDER BY created_at DESC, rowid DESC"#,
            account
        )
        .fetch_all(&self.pool)
        .await
    }

    /// The people `account` invited, in the order they joined.
    pub async fn invitees(&self, account: &MemberName) -> sqlx::Result<Vec<Invitee>> {
        sqlx::query_as!(
            Invitee,
            r#"SELECT account AS "account!: MemberName", joined_at, removed_at IS NOT NULL AS "removed!: bool"
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
            r#"SELECT account AS "account!: MemberName", invited_by AS "invited_by: MemberName",
                      joined_at,
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

    /// Adds `member`, marks the claimed invite redeemed by them and, if a
    /// member invited them, records their arrival for the greeter, all or
    /// none.
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
        // Root invites only bootstrap the network, so nobody's there to
        // introduce their redeemer.
        if member.invited_by.is_some() {
            sqlx::query!("INSERT INTO arrivals (account) VALUES (?)", member.account)
                .execute(&mut *transaction)
                .await?;
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
        let code_challenge = code.code_challenge.as_str();
        sqlx::query!(
            "INSERT INTO oauth_codes
                 (code_hash, session_hash, client_id, redirect_uri, code_challenge, expires_at)
             VALUES (?, ?, ?, ?, ?, ?)",
            code.code_hash,
            code.session_hash,
            code.client_id,
            code.redirect_uri,
            code_challenge,
            code.expires_at,
        )
        .execute(&self.pool)
        .await
        .map(drop)
    }

    /// Removes the code that hashes to `code_hash` and answers it, so no
    /// code is used twice, even by requests racing each other. A code whose
    /// stored challenge isn't one is gone all the same, and redeems nothing.
    pub async fn take_authorization_code(
        &self,
        code_hash: &AuthorizationCodeHash,
    ) -> sqlx::Result<Option<AuthorizationCode>> {
        let row = sqlx::query!(
            r#"DELETE FROM oauth_codes WHERE code_hash = ?
               RETURNING session_hash AS "session_hash: SessionHash", client_id, redirect_uri,
                         code_challenge, expires_at"#,
            code_hash
        )
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.and_then(|row| {
            Some(AuthorizationCode {
                code_challenge: CodeChallenge::parse(&row.code_challenge)?,
                session_hash: row.session_hash,
                client_id: row.client_id,
                redirect_uri: row.redirect_uri,
                expires_at: row.expires_at,
            })
        }))
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
        token_hash: &AccessTokenHash,
        now: i64,
    ) -> sqlx::Result<Option<AccessToken>> {
        sqlx::query_as!(
            AccessToken,
            r#"SELECT members.account AS "account!: MemberName", oauth_tokens.client_id,
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

    /// Whether a member, removed or not, holds `account`, in any case.
    /// Removed members keep their names, even once Ergo has erased their
    /// accounts and would hand the names out again.
    pub async fn held_by_member(&self, account: &str) -> sqlx::Result<bool> {
        sqlx::query_scalar!(
            r#"SELECT EXISTS (SELECT 1 FROM members WHERE account = ?) AS "held!: bool""#,
            account
        )
        .fetch_one(&self.pool)
        .await
    }

    /// Inserts `bot`, taking the row of `replacing`, the deleted bot whose
    /// name it reuses, unless its owner has `max_live` live bots already.
    /// Concurrent calls take turns, so they can't both see room for one
    /// more, and a name another bot took, or a deleted bot that changed,
    /// since the caller looked stays theirs.
    pub async fn insert_bot(
        &self,
        bot: &NewBot<'_>,
        replacing: Option<&Bot>,
        max_live: Option<i64>,
    ) -> sqlx::Result<BotInsertion> {
        let mut transaction = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        if let Some(max_live) = max_live {
            let live = sqlx::query_scalar!(
                r#"SELECT COUNT(*) AS "live!: i64" FROM bots
                   WHERE owner = ? COLLATE NOCASE AND removed_at IS NULL"#,
                bot.owner,
            )
            .fetch_one(&mut *transaction)
            .await?;
            if live >= max_live {
                return Ok(BotInsertion::TooMany);
            }
        }
        let replaced_owner = replacing.map(|replaced| &replaced.owner);
        let replaced_at = replacing.and_then(|replaced| replaced.removed_at);
        let inserted = sqlx::query!(
            "INSERT INTO bots (account, owner, note, created_at) VALUES (?, ?, ?, ?)
             ON CONFLICT (account) DO UPDATE
             SET owner = excluded.owner, note = excluded.note,
                 created_at = excluded.created_at, removed_at = NULL
             WHERE bots.owner = ? AND bots.removed_at = ?",
            bot.account,
            bot.owner,
            bot.note,
            bot.created_at,
            replaced_owner,
            replaced_at,
        )
        .execute(&mut *transaction)
        .await?;
        if inserted.rows_affected() == 0 {
            return Ok(BotInsertion::NameTaken);
        }
        transaction.commit().await?;
        Ok(BotInsertion::Inserted)
    }

    /// The bot whose account is `account`, in any case, deleted or not.
    pub async fn bot(&self, account: &str) -> sqlx::Result<Option<Bot>> {
        sqlx::query_as!(
            Bot,
            r#"SELECT account AS "account!: BotName", owner AS "owner: MemberName", note,
                      created_at, removed_at
               FROM bots WHERE account = ?"#,
            account
        )
        .fetch_optional(&self.pool)
        .await
    }

    /// The bots that haven't been deleted, of `owner` or of everyone, by
    /// name.
    pub async fn bots(&self, owner: Option<&MemberName>) -> sqlx::Result<Vec<Bot>> {
        sqlx::query_as!(
            Bot,
            r#"SELECT account AS "account!: BotName", owner AS "owner: MemberName", note,
                      created_at, removed_at
               FROM bots WHERE removed_at IS NULL AND (?1 IS NULL OR owner = ?1 COLLATE NOCASE)
               ORDER BY account"#,
            owner
        )
        .fetch_all(&self.pool)
        .await
    }

    /// Takes back the insertion of the bot `account` Ergo never created:
    /// drops its row, freeing its name, or puts back the deleted bot it
    /// replaced, which holds the name as it did.
    pub async fn take_back_bot(
        &self,
        account: &BotName,
        replaced: Option<&Bot>,
    ) -> sqlx::Result<()> {
        match replaced {
            None => {
                sqlx::query!(
                    "DELETE FROM bots WHERE account = ? AND removed_at IS NULL",
                    account
                )
                .execute(&self.pool)
                .await?
            }
            Some(replaced) => {
                sqlx::query!(
                    "UPDATE bots SET owner = ?, note = ?, created_at = ?, removed_at = ?
                     WHERE account = ? AND removed_at IS NULL",
                    replaced.owner,
                    replaced.note,
                    replaced.created_at,
                    replaced.removed_at,
                    account,
                )
                .execute(&self.pool)
                .await?
            }
        };
        Ok(())
    }

    /// Every bot's account, deleted ones included.
    pub async fn bot_accounts(&self) -> sqlx::Result<Vec<BotAccount>> {
        sqlx::query_as!(
            BotAccount,
            r#"SELECT bots.account AS "account!: BotName", bots.owner AS "owner: MemberName",
                      bots.removed_at IS NOT NULL AS "removed!: bool",
                      members.removed_at IS NOT NULL AS "owner_removed!: bool"
               FROM bots JOIN members ON members.account = bots.owner
               ORDER BY bots.account"#
        )
        .fetch_all(&self.pool)
        .await
    }

    /// Marks a bot deleted, keeping its row so its name stays held until
    /// it's free; answers whether it was live.
    pub async fn remove_bot(&self, account: &BotName, now: i64) -> sqlx::Result<bool> {
        let removed = sqlx::query!(
            "UPDATE bots SET removed_at = ? WHERE account = ? AND removed_at IS NULL",
            now,
            account,
        )
        .execute(&self.pool)
        .await?;
        Ok(removed.rows_affected() == 1)
    }

    /// The newcomers whose welcome is on record, in the order they joined.
    pub async fn arrivals(&self) -> sqlx::Result<Vec<Arrival>> {
        sqlx::query_as!(
            Arrival,
            r#"SELECT arrivals.account AS "account!: MemberName",
                      newcomer.invited_by AS "inviter!: MemberName",
                      newcomer.joined_at, arrivals.arrived_at, arrivals.announced_at,
                      arrivals.inviter_told_at, arrivals.nudged_at, arrivals.oriented_at
               FROM arrivals
               JOIN members newcomer ON newcomer.account = arrivals.account
               WHERE newcomer.invited_by IS NOT NULL
               ORDER BY newcomer.joined_at, arrivals.account"#
        )
        .fetch_all(&self.pool)
        .await
    }

    /// Deletes the arrivals of newcomers who joined at or before
    /// `joined_by`, answering how many.
    pub async fn forget_arrivals(&self, joined_by: i64) -> sqlx::Result<u64> {
        let deleted = sqlx::query!(
            "DELETE FROM arrivals WHERE account IN
                 (SELECT account FROM members WHERE joined_at <= ?)",
            joined_by
        )
        .execute(&self.pool)
        .await?;
        Ok(deleted.rows_affected())
    }

    /// Notes when `account`'s welcome reached `milestone`.
    pub async fn record(
        &self,
        account: &MemberName,
        milestone: Milestone<'_>,
        at: i64,
    ) -> sqlx::Result<()> {
        let pool = &self.pool;
        match milestone {
            Milestone::Arrived => {
                sqlx::query!(
                    "UPDATE arrivals SET arrived_at = ? WHERE account = ?",
                    at,
                    account
                )
                .execute(pool)
                .await
            }
            Milestone::Announced(msgid) => {
                sqlx::query!(
                    "UPDATE arrivals SET announced_at = ?, announce_msgid = ? WHERE account = ?",
                    at,
                    msgid,
                    account
                )
                .execute(pool)
                .await
            }
            Milestone::InviterTold => {
                sqlx::query!(
                    "UPDATE arrivals SET inviter_told_at = ? WHERE account = ?",
                    at,
                    account
                )
                .execute(pool)
                .await
            }
            Milestone::Oriented => {
                sqlx::query!(
                    "UPDATE arrivals SET oriented_at = ? WHERE account = ?",
                    at,
                    account
                )
                .execute(pool)
                .await
            }
            Milestone::Nudged => {
                sqlx::query!(
                    "UPDATE arrivals SET nudged_at = ? WHERE account = ?",
                    at,
                    account
                )
                .execute(pool)
                .await
            }
        }
        .map(drop)
    }

    /// Whether the reminder of the week named `week` went out.
    pub async fn weekly_reminder_sent(&self, week: &str) -> sqlx::Result<bool> {
        sqlx::query_scalar!(
            r#"SELECT EXISTS (SELECT 1 FROM weekly_reminders WHERE week = ?) AS "sent!: bool""#,
            week
        )
        .fetch_one(&self.pool)
        .await
    }

    pub async fn record_weekly_reminder(&self, week: &str, at: i64) -> sqlx::Result<()> {
        sqlx::query!(
            "INSERT INTO weekly_reminders (week, sent_at) VALUES (?, ?)",
            week,
            at
        )
        .execute(&self.pool)
        .await
        .map(drop)
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

async fn verification_code(
    executor: impl SqliteExecutor<'_>,
    code_hash: &VerificationCodeHash,
) -> sqlx::Result<Option<VerificationCode>> {
    sqlx::query_as!(
        VerificationCode,
        r#"SELECT account AS "account: MemberName", email AS "email: Email", created_at,
                  expires_at, replaced_at
           FROM verification_codes WHERE code_hash = ?"#,
        code_hash
    )
    .fetch_optional(executor)
    .await
}

async fn delete_verification_codes(
    executor: impl SqliteExecutor<'_>,
    account: &MemberName,
) -> sqlx::Result<()> {
    sqlx::query!("DELETE FROM verification_codes WHERE account = ?", account)
        .execute(executor)
        .await
        .map(drop)
}

async fn insert_invite(
    executor: impl SqliteExecutor<'_>,
    invite: &NewInvite<'_>,
) -> sqlx::Result<()> {
    let code_hash = InviteCodeHash::of(invite.code);
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
        let hash = InviteCodeHash::of(&String::from_utf8_lossy(&code));
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
    account: &MemberName,
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

async fn delete_sessions(
    executor: impl SqliteExecutor<'_>,
    account: &MemberName,
) -> sqlx::Result<u64> {
    let deleted = sqlx::query!("DELETE FROM sessions WHERE account = ?", account)
        .execute(executor)
        .await?;
    Ok(deleted.rows_affected())
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::secret_hash::SecretHash;
    use common::sqlite::testing::files_contain;
    use sqlx::sqlite::SqliteConnectOptions;
    use std::sync::LazyLock;

    pub fn email(address: &str) -> Email {
        Email::parse(address).unwrap()
    }

    pub async fn temp_db() -> (Db, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::migrate(&dir.path().join("concierge.db")).await.unwrap();
        (db, dir)
    }

    fn latest_migration() -> i64 {
        sqlite::latest_migration(&MIGRATOR)
    }

    pub(crate) async fn execute(db: &Db, sql: &str) -> sqlx::Result<()> {
        sqlx::query(sql).execute(&db.pool).await.map(drop)
    }

    #[tokio::test]
    async fn applies_every_migration_once() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("concierge.db");
        Db::migrate(&path).await.unwrap();
        Db::migrate(&path).await.unwrap();
        let reopened = Db::open(&path).await.unwrap();
        assert_eq!(
            reopened.migration_version().await.unwrap(),
            Some(latest_migration())
        );
    }

    #[tokio::test]
    async fn opening_needs_a_database_migrated_by_this_build() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("concierge.db");
        let error = Db::open(&path).await.err().unwrap();
        assert!(
            format!("{error:#}").contains("can't open the database"),
            "{error:#}"
        );
        assert!(!path.exists());

        written_by_version(&path, 20261019120000, "").await;
        let error = Db::open(&path).await.err().unwrap();

        assert_eq!(
            error.to_string(),
            format!(
                "the database {} is at migration 20261019120000, not {}; \
                 `concierge migrate` brings it up to date",
                path.display(),
                latest_migration(),
            )
        );
        Db::migrate(&path).await.unwrap();
        Db::open(&path).await.unwrap();
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

        migrating_keeps_alice_bob_and_what_refers_to_them(&path).await;
    }

    #[tokio::test]
    async fn requiring_an_email_keeps_members_and_whatever_refers_to_them() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("concierge.db");
        let open: String = InviteCodeHash::of("open")
            .as_bytes()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        written_by_version(
            &path,
            20261020120000,
            &format!(
                "INSERT INTO members (account, email, joined_at, is_admin)
                 VALUES ('alice', 'alice@example.org', 1, TRUE);
                 INSERT INTO members (account, invited_by, joined_at, removed_at)
                 VALUES ('bob', 'alice', 2, 3);
                 INSERT INTO invites (code_hash, created_by, created_at, expires_at)
                 VALUES (x'{open}', 'alice', 4, 5);
                 INSERT INTO sessions (token_hash, account, created_at, expires_at)
                 VALUES (x'00', 'alice', 6, 70);
                 INSERT INTO oauth_tokens (token_hash, session_hash, client_id, created_at, expires_at)
                 VALUES (x'01', x'00', 'gamja', 6, 70);
                 INSERT INTO bots (account, owner, created_at) VALUES ('bender', 'alice', 8);"
            ),
        )
        .await;

        migrating_keeps_alice_bob_and_what_refers_to_them(&path).await;
    }

    /// Migrates the database at `path`, where alice runs bender, holds a
    /// session with a token and made the invite "open", and bob, whom she
    /// invited, was removed; checks all of it is still there.
    async fn migrating_keeps_alice_bob_and_what_refers_to_them(path: &Path) {
        let db = Db::migrate(path).await.unwrap();

        assert_eq!(
            db.session(&hash(b"\x00"), 10)
                .await
                .unwrap()
                .unwrap()
                .member
                .account
                .as_str(),
            "alice"
        );
        assert!(db.access_token(&hash(b"\x01"), 10).await.unwrap().is_some());

        let alice = db.member("ALICE").await.unwrap().unwrap();
        assert_eq!(
            alice.email.as_ref().map(Email::as_str),
            Some("alice@example.org")
        );
        assert!(alice.is_admin);
        let bob = db.member("bob").await.unwrap().unwrap();
        assert_eq!(
            (
                bob.invited_by.as_ref().map(MemberName::as_str),
                bob.removed_at
            ),
            (Some("alice"), Some(3))
        );
        assert!(db.invite("open").await.unwrap().is_some());
        assert_eq!(db.bots(Some(&alice.account)).await.unwrap().len(), 1);
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
            "INSERT INTO members (account, email, joined_at) VALUES ('alice', 'alice@example.org', 0);
             INSERT INTO invites (code, created_by, created_at, expires_at)
             VALUES ('q7Hx2mZp9vLcT4wRfBa-_w', 'alice', 1, 2), ('v0Ri8dEa2kWnL5yTcHq-_x', NULL, 3, 4);",
        )
        .await;

        let db = Db::migrate(&path).await.unwrap();

        let invite = db.invite("q7Hx2mZp9vLcT4wRfBa-_w").await.unwrap().unwrap();
        assert_eq!(
            invite.created_by.as_ref().map(MemberName::as_str),
            Some("alice")
        );
        assert!(db.invite("v0Ri8dEa2kWnL5yTcHq-_x").await.unwrap().is_some());
        let kept: Vec<Vec<u8>> = sqlx::query_scalar("SELECT code_hash FROM invites ORDER BY rowid")
            .fetch_all(&db.pool)
            .await
            .unwrap();
        let hashes = ["q7Hx2mZp9vLcT4wRfBa-_w", "v0Ri8dEa2kWnL5yTcHq-_x"]
            .map(|code| InviteCodeHash::of(code).as_bytes().to_vec());
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
        let hashed = InviteCodeHash::of("v0Ri8dEa2kWnL5yTcHq-_x");
        let pool = SqlitePool::connect(&format!("sqlite://{}", path.display()))
            .await
            .unwrap();
        sqlx::query("INSERT INTO invites (code_hash, created_at, expires_at) VALUES (?, 3, 4)")
            .bind(&hashed)
            .execute(&pool)
            .await
            .unwrap();
        pool.close().await;

        Db::migrate(&path).await.unwrap();
        let db = Db::migrate(&path).await.unwrap();

        let kept: Vec<Vec<u8>> = sqlx::query_scalar("SELECT code_hash FROM invites ORDER BY rowid")
            .fetch_all(&db.pool)
            .await
            .unwrap();
        let carried_over = InviteCodeHash::of("q7Hx2mZp9vLcT4wRfBa-_w");
        assert_eq!(kept, [carried_over.as_bytes(), hashed.as_bytes()]);
    }

    #[tokio::test]
    async fn a_database_whose_references_dont_hold_fails_to_migrate() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("concierge.db");
        written_by_version(
            &path,
            20261016120000,
            "PRAGMA foreign_keys = OFF;
             INSERT INTO sessions (token_hash, account, created_at, expires_at) VALUES (x'00', 'ghost', 0, 1);",
        )
        .await;

        let error = Db::migrate(&path).await.err().unwrap();

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
        let insert = |name: &str| {
            format!(
                "INSERT INTO members (account, email, joined_at) VALUES ('{name}', 'a@example.org', 0)"
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
    async fn a_current_member_has_an_email() {
        let (db, _dir) = temp_db().await;
        let error = execute(
            &db,
            "INSERT INTO members (account, joined_at) VALUES ('alice', 0)",
        )
        .await
        .unwrap_err();
        assert!(error.to_string().contains("CHECK"), "{error}");
    }

    #[tokio::test]
    async fn a_verified_address_belongs_to_one_member_whatever_its_case() {
        let (db, _dir) = temp_db().await;
        execute(
            &db,
            "INSERT INTO members (account, email, email_verified_at, joined_at)
             VALUES ('alice', 'Alice@Example.org', 1, 0),
                    ('bob', 'alice@example.org', NULL, 0),
                    ('carol', 'ALICE@EXAMPLE.ORG', NULL, 0),
                    ('dave', 'a.lice+irc@example.org', 1, 0);",
        )
        .await
        .unwrap();
        let error = execute(
            &db,
            "UPDATE members SET email_verified_at = 2 WHERE account = 'bob'",
        )
        .await
        .unwrap_err();
        assert!(error.to_string().contains("UNIQUE"), "{error}");
        db.remove_member(&MemberName::new("alice"), 3)
            .await
            .unwrap();
        execute(
            &db,
            "UPDATE members SET email_verified_at = 4 WHERE account = 'bob'",
        )
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn a_current_member_without_an_email_fails_the_migration() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("concierge.db");
        written_by_version(
            &path,
            20261020120000,
            "INSERT INTO members (account, joined_at) VALUES ('alice', 0);",
        )
        .await;

        let error = Db::migrate(&path).await.err().unwrap();

        assert!(format!("{error:#}").contains("CHECK"), "{error:#}");
    }

    #[tokio::test]
    async fn removing_a_member_keeps_the_invites_that_shaped_the_tree() {
        let (db, _dir) = temp_db().await;
        execute(
            &db,
            "INSERT INTO members (account, email, invited_by, joined_at)
             VALUES ('root', 'root@example.org', NULL, 0),
                    ('alice', 'alice@example.org', 'root', 10),
                    ('bob', 'bob@example.org', 'alice', 20);
             UPDATE members
             SET email_verified_at = 11, news_opt_in = TRUE, is_admin = TRUE,
                 verification_first_sent_at = 10, verification_reminded_at = 11
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

        let removal = db
            .remove_member(&MemberName::new("alice"), 200)
            .await
            .unwrap();

        assert_eq!(removal.invites_revoked, 1);
        let alice = db.member("alice").await.unwrap().unwrap();
        assert_eq!(
            alice,
            Member {
                account: MemberName::new("alice"),
                email: None,
                email_verified_at: None,
                news_opt_in: false,
                invited_by: Some(MemberName::new("root")),
                joined_at: 10,
                is_admin: false,
                removed_at: Some(200),
                verification_first_sent_at: None,
                verification_reminded_at: None,
            }
        );
        for (code, redeemed_by, revoked_at) in [
            ("open", None, Some(200)),
            ("stale", None, None),
            ("to-alice", Some("alice"), None),
            ("to-bob", Some("bob"), None),
        ] {
            let invite = db.invite(code).await.unwrap().unwrap();
            assert_eq!(
                invite.redeemed_by.as_ref().map(MemberName::as_str),
                redeemed_by,
                "{code}"
            );
            assert_eq!(invite.revoked_at, revoked_at, "{code}");
        }
        let bob = db.member("bob").await.unwrap().unwrap();
        assert_eq!(
            bob.invited_by.as_ref().map(MemberName::as_str),
            Some("alice")
        );
        assert_eq!(
            bob.email.as_ref().map(Email::as_str),
            Some("bob@example.org")
        );
    }

    #[tokio::test]
    async fn revoking_a_members_open_invites_leaves_every_other_invite_alone() {
        let (db, _dir) = temp_db().await;
        execute(
            &db,
            "INSERT INTO members (account, email, joined_at)
             VALUES ('alice', 'alice@example.org', 0), ('bob', 'bob@example.org', 0),
                    ('carol', 'carol@example.org', 0);",
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

        let alice = MemberName::new("alice");
        let revoked = db.revoke_open_invites(&alice, 200).await.unwrap();

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
    async fn a_removed_members_email_leaves_the_database_files() {
        let (db, dir) = temp_db().await;
        let secure_delete: i64 = sqlx::query_scalar("PRAGMA secure_delete")
            .fetch_one(&db.pool)
            .await
            .unwrap();
        assert_eq!(secure_delete, 1);
        execute(
            &db,
            "INSERT INTO members (account, email, joined_at)
             VALUES ('alice', 'alice@example.org', 10),
                    ('bob', 'bob@example.org', 20);",
        )
        .await
        .unwrap();
        let alices = issue(&db, "alice", 100).await;
        let bobs = issue(&db, "bob", 100).await;

        let removal = db
            .remove_member(&MemberName::new("alice"), 200)
            .await
            .unwrap();

        assert!(removal.off_the_disk);
        assert!(!files_contain(&dir, b"alice@example.org"));
        assert!(files_contain(&dir, b"bob@example.org"));
        assert_eq!(
            db.verification_code(alices.code.expose()).await.unwrap(),
            None
        );
        assert!(
            db.verification_code(bobs.code.expose())
                .await
                .unwrap()
                .is_some()
        );
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

        let removal = db
            .remove_member(&MemberName::new("alice"), 200)
            .await
            .unwrap();

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
        let created_by = created_by.map(MemberName::new);
        let invite = NewInvite {
            code,
            created_by: created_by.as_ref(),
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
        .bind(InviteCodeHash::of(code))
        .execute(&db.pool)
        .await
        .unwrap();
    }

    /// The accounts with an arrival on record, which no query of the
    /// site's lists.
    async fn arrivals_recorded(db: &Db) -> Vec<String> {
        sqlx::query_scalar("SELECT account FROM arrivals ORDER BY account")
            .fetch_all(&db.pool)
            .await
            .unwrap()
    }

    /// alice invited someone with the code "open", which nobody used yet.
    async fn with_open_invite() -> (Db, tempfile::TempDir) {
        let (db, dir) = temp_db().await;
        execute(
            &db,
            "INSERT INTO members (account, email, joined_at) VALUES ('alice', 'alice@example.org', 0)",
        )
        .await
        .unwrap();
        let expires_at = invite_rules::expires_at(CREATED);
        add_invite(&db, "open", Some("alice"), CREATED, expires_at).await;
        (db, dir)
    }

    static BOBS_EMAIL: LazyLock<Email> = LazyLock::new(|| email("bob@example.org"));
    static ALICE: LazyLock<MemberName> = LazyLock::new(|| MemberName::new("alice"));

    fn newcomer(account: &MemberName, joined_at: i64) -> NewMember<'_> {
        NewMember {
            account,
            email: &BOBS_EMAIL,
            news_opt_in: true,
            invited_by: Some(&ALICE),
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
                code_hash: InviteCodeHash::of("open"),
                claimed_at: now,
                created_by: Some(ALICE.clone()),
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
        let redeemed = db
            .redeem_invite(&stale, &newcomer(&MemberName::new("bob"), later))
            .await;
        assert_eq!(redeemed.unwrap(), Redemption::Lost);
        assert_eq!(db.member("bob").await.unwrap(), None);
        assert!(arrivals_recorded(&db).await.is_empty());

        let redeemed = db
            .redeem_invite(&fresh, &newcomer(&MemberName::new("carol"), later))
            .await;
        assert_eq!(redeemed.unwrap(), Redemption::Redeemed);
        assert_eq!(arrivals_recorded(&db).await, ["carol"]);
    }

    #[tokio::test]
    async fn redeeming_adds_the_member_and_uses_the_invite_up() {
        let (db, _dir) = with_open_invite().await;
        let now = CREATED + 10;
        let claim = db.claim_invite("open", now).await.unwrap().unwrap();

        let redeemed = db
            .redeem_invite(&claim, &newcomer(&MemberName::new("Bob"), now))
            .await;

        assert_eq!(redeemed.unwrap(), Redemption::Redeemed);
        let bob = db.member("bob").await.unwrap().unwrap();
        assert_eq!(
            bob.invited_by.as_ref().map(MemberName::as_str),
            Some("alice")
        );
        assert_eq!(
            bob.email.as_ref().map(Email::as_str),
            Some("bob@example.org")
        );
        assert!(bob.news_opt_in);
        let invite = db.invite("open").await.unwrap().unwrap();
        assert_eq!(
            invite.redeemed_by.as_ref().map(MemberName::as_str),
            Some("Bob")
        );
        assert_eq!(invite.status(now), invite_rules::Status::Redeemed);
        assert_eq!(arrivals_recorded(&db).await, ["Bob"]);
        db.release_claim(&claim).await.unwrap();
        assert_eq!(db.claim_invite("open", now + 1_000).await.unwrap(), None);
    }

    #[tokio::test]
    async fn an_invite_revoked_mid_redemption_adds_nobody() {
        let (db, _dir) = with_open_invite().await;
        let claim = db.claim_invite("open", CREATED).await.unwrap().unwrap();
        update_invite(&db, "open", "revoked_at = 1").await;
        let redeemed = db
            .redeem_invite(&claim, &newcomer(&MemberName::new("bob"), CREATED))
            .await;
        assert_eq!(redeemed.unwrap(), Redemption::Lost);
        assert_eq!(db.member("bob").await.unwrap(), None);
        assert!(arrivals_recorded(&db).await.is_empty());
    }

    #[tokio::test]
    async fn a_taken_name_fails_the_redemption_and_leaves_the_invite_unused() {
        let (db, _dir) = with_open_invite().await;
        let claim = db.claim_invite("open", CREATED).await.unwrap().unwrap();
        assert!(
            db.redeem_invite(&claim, &newcomer(&MemberName::new("ALICE"), CREATED))
                .await
                .is_err()
        );
        assert_eq!(db.invite("open").await.unwrap().unwrap().redeemed_at, None);
        assert!(arrivals_recorded(&db).await.is_empty());
    }

    #[tokio::test]
    async fn a_root_invite_records_no_arrival() {
        let (db, _dir) = with_open_invite().await;
        add_invite(
            &db,
            "root",
            None,
            CREATED,
            invite_rules::expires_at(CREATED),
        )
        .await;
        let claim = db.claim_invite("root", CREATED).await.unwrap().unwrap();
        let bob = MemberName::new("bob");
        let root_invitee = NewMember {
            invited_by: None,
            ..newcomer(&bob, CREATED)
        };

        let redeemed = db.redeem_invite(&claim, &root_invitee).await.unwrap();

        assert_eq!(redeemed, Redemption::Redeemed);
        assert!(arrivals_recorded(&db).await.is_empty());
    }

    #[tokio::test]
    async fn removing_a_newcomer_takes_their_arrival_with_them() {
        let (db, _dir) = with_open_invite().await;
        let claim = db.claim_invite("open", CREATED).await.unwrap().unwrap();
        let bob = MemberName::new("bob");
        db.redeem_invite(&claim, &newcomer(&bob, CREATED))
            .await
            .unwrap();

        db.remove_member(&bob, CREATED + 60).await.unwrap();

        assert!(arrivals_recorded(&db).await.is_empty());
    }

    #[tokio::test]
    async fn lists_arrivals_in_joining_order_with_their_inviters() {
        let (db, _dir) = temp_db().await;
        execute(
            &db,
            "INSERT INTO members (account, email, invited_by, joined_at)
             VALUES ('root', 'root@example.org', NULL, 0),
                    ('alice', 'alice@example.org', 'root', 10),
                    ('gone', 'gone@example.org', 'root', 10),
                    ('erin', 'erin@example.org', 'alice', 30),
                    ('dave', 'dave@example.org', 'gone', 20),
                    ('carol', 'carol@example.org', 'alice', 40);
             INSERT INTO arrivals (account) VALUES ('erin'), ('dave'), ('carol');",
        )
        .await
        .unwrap();
        db.remove_member(&MemberName::new("gone"), 25)
            .await
            .unwrap();
        assert_eq!(arrivals_recorded(&db).await, ["carol", "erin"]);
        let erin = MemberName::new("erin");
        db.record(&erin, Milestone::Arrived, 35).await.unwrap();
        db.record(&erin, Milestone::Announced(Some("msg-1")), 36)
            .await
            .unwrap();
        db.record(&erin, Milestone::InviterTold, 37).await.unwrap();
        db.record(&erin, Milestone::Oriented, 38).await.unwrap();
        db.record(&MemberName::new("carol"), Milestone::Nudged, 50)
            .await
            .unwrap();

        let arrivals = db.arrivals().await.unwrap();

        let arrival = |account: &str, joined_at| Arrival {
            account: MemberName::new(account),
            inviter: MemberName::new("alice"),
            joined_at,
            arrived_at: None,
            announced_at: None,
            inviter_told_at: None,
            nudged_at: None,
            oriented_at: None,
        };
        assert_eq!(
            arrivals,
            [
                Arrival {
                    arrived_at: Some(35),
                    announced_at: Some(36),
                    inviter_told_at: Some(37),
                    oriented_at: Some(38),
                    ..arrival("erin", 30)
                },
                Arrival {
                    nudged_at: Some(50),
                    ..arrival("carol", 40)
                },
            ]
        );
        let msgid: Option<String> =
            sqlx::query_scalar("SELECT announce_msgid FROM arrivals WHERE account = 'erin'")
                .fetch_one(&db.pool)
                .await
                .unwrap();
        assert_eq!(msgid.as_deref(), Some("msg-1"));
    }

    #[tokio::test]
    async fn forgets_the_arrivals_of_newcomers_who_joined_by_a_time() {
        let (db, _dir) = temp_db().await;
        execute(
            &db,
            "INSERT INTO members (account, email, joined_at)
             VALUES ('dave', 'dave@example.org', 20), ('erin', 'erin@example.org', 30);
             INSERT INTO arrivals (account) VALUES ('dave'), ('erin');",
        )
        .await
        .unwrap();

        assert_eq!(db.forget_arrivals(20).await.unwrap(), 1);

        assert_eq!(arrivals_recorded(&db).await, ["erin"]);
        assert!(db.member("dave").await.unwrap().is_some());
    }

    #[tokio::test]
    async fn remembers_which_weeks_were_reminded() {
        let (db, _dir) = temp_db().await;
        assert!(!db.weekly_reminder_sent("2026-10-26").await.unwrap());

        db.record_weekly_reminder("2026-10-26", 1_793_034_000)
            .await
            .unwrap();

        assert!(db.weekly_reminder_sent("2026-10-26").await.unwrap());
        assert!(!db.weekly_reminder_sent("2026-11-02").await.unwrap());
    }

    #[tokio::test]
    async fn members_from_before_arrivals_are_never_welcomed() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("concierge.db");
        written_by_version(
            &path,
            20261030120000,
            "INSERT INTO members (account, email, joined_at) VALUES ('alice', 'alice@example.org', 0);",
        )
        .await;

        let db = Db::migrate(&path).await.unwrap();

        assert!(arrivals_recorded(&db).await.is_empty());
    }

    #[tokio::test]
    async fn lists_a_members_invites_and_the_people_they_brought() {
        let (db, _dir) = with_open_invite().await;
        let claim = db.claim_invite("open", CREATED).await.unwrap().unwrap();
        db.redeem_invite(&claim, &newcomer(&MemberName::new("bob"), CREATED + 5))
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

        let invites = db.invites_created_by(&ALICE).await.unwrap();
        let hashes: Vec<_> = invites.into_iter().map(|invite| invite.code_hash).collect();
        let newest_first = ["made-in-the-same-second", "newer", "open"].map(InviteCodeHash::of);
        assert_eq!(hashes, newest_first);
        assert_eq!(
            db.invitees(&ALICE).await.unwrap(),
            [Invitee {
                account: MemberName::new("bob"),
                joined_at: CREATED + 5,
                removed: false,
            }]
        );
        let bob = MemberName::new("bob");
        assert!(db.invitees(&bob).await.unwrap().is_empty());
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
            "INSERT INTO members (account, email, joined_at)
             VALUES ('alice', 'alice@example.org', 0), ('bob', 'bob@example.org', 0);
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
        let session = db.session(&hash(b"\xa1"), 99).await.unwrap().unwrap();
        assert_eq!(
            (session.member.account.as_str(), session.expires_at),
            ("alice", 100)
        );
        assert_eq!(db.session(&hash(b"\xa1"), 100).await.unwrap(), None);
        assert_eq!(db.session(&hash(b"\xff"), 0).await.unwrap(), None);
    }

    #[tokio::test]
    async fn renewing_pushes_the_expiry_back() {
        let (db, _dir) = with_sessions().await;
        db.renew_session(&hash(b"\xa1"), 500).await.unwrap();
        assert_eq!(
            db.session(&hash(b"\xa1"), 300)
                .await
                .unwrap()
                .unwrap()
                .expires_at,
            500
        );
        assert_eq!(
            db.session(&hash(b"\xb1"), 0)
                .await
                .unwrap()
                .unwrap()
                .expires_at,
            100
        );
    }

    #[tokio::test]
    async fn ending_a_session_leaves_the_members_others() {
        let (db, _dir) = with_sessions().await;
        db.end_session(&hash(b"\xa1")).await.unwrap();
        assert_eq!(db.session(&hash(b"\xa1"), 0).await.unwrap(), None);
        assert!(db.session(&hash(b"\xa2"), 0).await.unwrap().is_some());
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
        assert_eq!(
            db.end_other_sessions(&MemberName::new("alice"), &hash(b"\xa2"))
                .await
                .unwrap(),
            1
        );
        assert_eq!(db.session(&hash(b"\xa1"), 0).await.unwrap(), None);
        assert!(db.session(&hash(b"\xa2"), 0).await.unwrap().is_some());
        assert!(db.session(&hash(b"\xb1"), 0).await.unwrap().is_some());
        assert_eq!(db.access_token(&hash(b"\xf1"), 0).await.unwrap(), None);
        assert!(db.access_token(&hash(b"\xf2"), 0).await.unwrap().is_some());
    }

    #[tokio::test]
    async fn expired_sessions_are_cleaned_up() {
        let (db, _dir) = with_sessions().await;
        assert_eq!(db.end_expired_sessions(150).await.unwrap(), 2);
        assert!(db.session(&hash(b"\xa2"), 150).await.unwrap().is_some());
        assert_eq!(db.end_sessions(&MemberName::new("bob")).await.unwrap(), 0);
    }

    #[tokio::test]
    async fn an_address_is_taken_once_another_member_verified_it_in_any_case() {
        let (db, _dir) = with_unverified_members().await;
        for (email, account, taken) in [
            ("CAROL@Example.org", "alice", true),
            ("carol@example.org", "newcomer", true),
            ("carol@example.org", "Carol", false),
            ("bob@example.org", "alice", false),
            ("carol+irc@example.org", "alice", false),
        ] {
            let found = db
                .email_taken(&self::email(email), &MemberName::new(account))
                .await
                .unwrap();
            assert_eq!(found, taken, "{email} for {account}");
        }
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
        db.update_settings(&MemberName::new("alice"), &email("alice@example.org"), true)
            .await
            .unwrap();
        let alice = db.member("alice").await.unwrap().unwrap();
        assert_eq!(
            (alice.email_verified_at, alice.news_opt_in),
            (Some(10), true)
        );
        db.update_settings(
            &MemberName::new("alice"),
            &email("alice@example.net"),
            false,
        )
        .await
        .unwrap();
        let alice = db.member("alice").await.unwrap().unwrap();
        assert_eq!(
            alice.email.as_ref().map(Email::as_str),
            Some("alice@example.net")
        );
        assert_eq!((alice.email_verified_at, alice.news_opt_in), (None, false));
    }

    const MAILED: i64 = 1_760_000_000;

    /// alice and bob, who haven't verified their emails, and carol, who has.
    async fn with_unverified_members() -> (Db, tempfile::TempDir) {
        let (db, dir) = temp_db().await;
        execute(
            &db,
            "INSERT INTO members (account, email, email_verified_at, joined_at)
             VALUES ('Alice', 'alice@example.org', NULL, 0),
                    ('bob', 'bob@example.org', NULL, 0),
                    ('carol', 'carol@example.org', 1, 0);",
        )
        .await
        .unwrap();
        (db, dir)
    }

    async fn issue(db: &Db, account: &str, now: i64) -> IssuedCode {
        db.issue_verification_code(&MemberName::new(account), now)
            .await
            .unwrap()
            .unwrap()
    }

    #[tokio::test]
    async fn an_issued_codes_debug_output_leaves_the_code_out() {
        let (db, _dir) = with_unverified_members().await;
        let issued = issue(&db, "alice", MAILED).await;
        let debug = format!("{issued:?}");
        assert!(!debug.contains(issued.code.expose().as_str()), "{debug}");
        assert!(debug.contains("alice@example.org"), "{debug}");
        assert!(debug.contains("code: <redacted>"), "{debug}");
    }

    #[tokio::test]
    async fn issuing_a_code_binds_it_to_the_member_and_their_address() {
        let (db, dir) = with_unverified_members().await;

        let issued = issue(&db, "alice", MAILED).await;

        assert_eq!(issued.code.expose().len(), link_code::MAILED_LENGTH);
        let expires_at = verification_rules::expires_at(MAILED);
        assert_eq!(
            (
                issued.account.as_str(),
                issued.email.as_str(),
                issued.expires_at
            ),
            ("Alice", "alice@example.org", expires_at)
        );
        assert_eq!(
            db.verification_code(issued.code.expose()).await.unwrap(),
            Some(VerificationCode {
                account: MemberName::new("Alice"),
                email: email("alice@example.org"),
                created_at: MAILED,
                expires_at,
                replaced_at: None,
            })
        );
        assert!(!files_contain(&dir, issued.code.expose().as_bytes()));
        let alice = db.member("alice").await.unwrap().unwrap();
        assert_eq!(alice.verification_first_sent_at, Some(MAILED));
    }

    #[tokio::test]
    async fn a_new_code_replaces_the_old_and_the_grace_period_keeps_its_start() {
        let (db, _dir) = with_unverified_members().await;
        let first = issue(&db, "alice", MAILED).await;
        let bobs = issue(&db, "bob", MAILED).await;

        let second = issue(&db, "alice", MAILED + 60).await;

        assert_ne!(first.code.expose(), second.code.expose());
        assert_eq!(first.deadline, verification_rules::deadline(MAILED));
        assert_eq!(second.deadline, first.deadline);
        let first = db
            .verification_code(first.code.expose())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(first.replaced_at, Some(MAILED + 60));
        assert_eq!(
            first.status(MAILED + 60),
            verification_rules::Status::Replaced
        );
        for live in [second.code, bobs.code] {
            let live = db.verification_code(live.expose()).await.unwrap().unwrap();
            assert_eq!(live.status(MAILED + 60), verification_rules::Status::Live);
        }
        let alice = db.member("alice").await.unwrap().unwrap();
        assert_eq!(alice.verification_first_sent_at, Some(MAILED));
    }

    #[tokio::test]
    async fn issuing_a_code_clears_out_expired_ones() {
        let (db, _dir) = with_unverified_members().await;
        let old = issue(&db, "alice", MAILED).await;
        let expired = verification_rules::expires_at(MAILED);
        let stale = db
            .verification_code(old.code.expose())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(stale.status(expired), verification_rules::Status::Expired);

        issue(&db, "bob", expired).await;

        assert_eq!(db.verification_code(old.code.expose()).await.unwrap(), None);
    }

    #[tokio::test]
    async fn verified_removed_and_unknown_members_get_no_code() {
        let (db, _dir) = with_unverified_members().await;
        db.remove_member(&MemberName::new("bob"), MAILED)
            .await
            .unwrap();
        for account in ["carol", "bob", "nobody"] {
            let issued = db
                .issue_verification_code(&MemberName::new(account), MAILED)
                .await
                .unwrap();
            assert!(issued.is_none(), "{account}");
        }
        let carol = db.member("carol").await.unwrap().unwrap();
        assert_eq!(carol.verification_first_sent_at, None);
    }

    #[tokio::test]
    async fn a_live_code_verifies_its_address_once() {
        let (db, _dir) = with_unverified_members().await;
        let replaced = issue(&db, "alice", MAILED).await;
        let issued = issue(&db, "alice", MAILED + 60).await;
        let bobs = issue(&db, "bob", MAILED).await;
        let now = issued.expires_at - 1;

        let verification = db.verify_email(issued.code.expose(), now).await.unwrap();

        assert!(
            matches!(&verification, Verification::Verified { account } if account.as_str() == "Alice"),
            "{verification:?}"
        );
        let alice = db.member("alice").await.unwrap().unwrap();
        assert_eq!(alice.email_verified_at, Some(now));
        for used_up in [issued.code.expose(), replaced.code.expose()] {
            assert_eq!(db.verification_code(used_up).await.unwrap(), None);
            let again = db.verify_email(used_up, now).await.unwrap();
            assert_eq!(again, Verification::Unknown);
        }
        assert!(
            db.verification_code(bobs.code.expose())
                .await
                .unwrap()
                .is_some()
        );
        assert_eq!(
            db.member("bob").await.unwrap().unwrap().email_verified_at,
            None
        );
    }

    #[tokio::test]
    async fn replaced_expired_and_unknown_codes_verify_nothing() {
        let (db, _dir) = with_unverified_members().await;
        let replaced = issue(&db, "alice", MAILED).await;
        let live = issue(&db, "alice", MAILED + 60).await;
        let expiring = issue(&db, "bob", MAILED).await;

        for (code, at, outcome) in [
            (
                replaced.code.expose().as_str(),
                MAILED + 60,
                Verification::Replaced,
            ),
            (
                expiring.code.expose().as_str(),
                expiring.expires_at,
                Verification::Expired,
            ),
            (&live.code.expose()[1..], MAILED + 60, Verification::Unknown),
        ] {
            assert_eq!(db.verify_email(code, at).await.unwrap(), outcome);
        }
        for account in ["alice", "bob"] {
            let member = db.member(account).await.unwrap().unwrap();
            assert_eq!(member.email_verified_at, None, "{account}");
        }
        assert!(
            db.verification_code(live.code.expose())
                .await
                .unwrap()
                .is_some()
        );
    }

    #[tokio::test]
    async fn a_code_verifies_nothing_once_its_member_has_another_address() {
        let (db, _dir) = with_unverified_members().await;
        let issued = issue(&db, "alice", MAILED).await;
        execute(
            &db,
            "UPDATE members SET email = 'alice@example.net' WHERE account = 'alice'",
        )
        .await
        .unwrap();

        let verification = db.verify_email(issued.code.expose(), MAILED).await.unwrap();

        assert_eq!(verification, Verification::AddressChanged);
        let alice = db.member("alice").await.unwrap().unwrap();
        assert_eq!(alice.email_verified_at, None);
    }

    #[tokio::test]
    async fn an_address_another_member_verified_first_stays_theirs() {
        let (db, _dir) = with_unverified_members().await;
        execute(
            &db,
            "UPDATE members SET email = 'Carol@Example.org' WHERE account = 'bob'",
        )
        .await
        .unwrap();
        let issued = issue(&db, "bob", MAILED).await;

        let verification = db.verify_email(issued.code.expose(), MAILED).await.unwrap();

        assert_eq!(verification, Verification::AddressTaken);
        let bob = db.member("bob").await.unwrap().unwrap();
        assert_eq!(bob.email_verified_at, None);
        assert!(
            db.verification_code(issued.code.expose())
                .await
                .unwrap()
                .is_some()
        );
    }

    #[tokio::test]
    async fn changing_the_email_drops_the_codes_sent_to_the_old_one() {
        let (db, _dir) = with_unverified_members().await;
        let kept = issue(&db, "Alice", MAILED).await;
        let bobs = issue(&db, "bob", MAILED).await;
        db.update_settings(&MemberName::new("Alice"), &email("alice@example.org"), true)
            .await
            .unwrap();
        assert!(
            db.verification_code(kept.code.expose())
                .await
                .unwrap()
                .is_some()
        );

        db.update_settings(&MemberName::new("Alice"), &email("alice@example.net"), true)
            .await
            .unwrap();

        let verification = db.verify_email(kept.code.expose(), MAILED).await.unwrap();
        assert_eq!(verification, Verification::Unknown);
        assert!(
            db.verification_code(bobs.code.expose())
                .await
                .unwrap()
                .is_some()
        );
    }

    #[tokio::test]
    async fn progress_follows_the_newest_link_and_the_reminder_of_current_members() {
        let (db, _dir) = with_unverified_members().await;
        execute(
            &db,
            "INSERT INTO members (account, email, joined_at, removed_at)
             VALUES ('dave', NULL, 0, 1)",
        )
        .await
        .unwrap();
        issue(&db, "alice", MAILED).await;
        issue(&db, "alice", MAILED + 60).await;
        db.record_reminder(&MemberName::new("alice"), MAILED + 60)
            .await
            .unwrap();

        let progress = db.verification_progress().await.unwrap();

        let summary: Vec<_> = progress
            .iter()
            .map(|(account, progress)| {
                (
                    account.as_str(),
                    progress.verified,
                    progress.first_sent_at,
                    progress.reminded_at,
                    progress.last_sent_at,
                )
            })
            .collect();
        assert_eq!(
            summary,
            [
                (
                    "Alice",
                    false,
                    Some(MAILED),
                    Some(MAILED + 60),
                    Some(MAILED + 60)
                ),
                ("bob", false, None, None, None),
                ("carol", true, None, None, None),
            ]
        );
    }

    #[tokio::test]
    async fn a_link_that_was_used_or_cleared_out_leaves_no_last_sent_at() {
        let (db, _dir) = with_unverified_members().await;
        let expired = verification_rules::expires_at(MAILED);
        issue(&db, "bob", MAILED).await;
        let alices = issue(&db, "alice", expired).await;
        db.verify_email(alices.code.expose(), expired)
            .await
            .unwrap();

        let progress = db.verification_progress().await.unwrap();

        for (account, progress) in &progress[..2] {
            assert_eq!(progress.last_sent_at, None, "{account}");
        }
    }

    mod reset {
        use super::*;

        /// carol, as `with_unverified_members` confirmed her email.
        fn carol() -> Recoverable {
            Recoverable {
                account: MemberName::new("carol"),
                email: email("carol@example.org"),
            }
        }

        async fn issue(db: &Db, account: &str, now: i64) -> IssuedReset {
            let account = MemberName::new(account);
            db.issue_reset_code(&account, now).await.unwrap().unwrap()
        }

        #[tokio::test]
        async fn finds_a_confirmed_member_by_nick_or_email_in_any_case() {
            let (db, _dir) = with_unverified_members().await;
            for input in ["carol", "CAROL", "carol@example.org", "Carol@EXAMPLE.org"] {
                let lookup = Lookup::parse(input).unwrap();
                let found = db.recoverable_member(&lookup).await.unwrap();
                let spelled = found.as_ref().map(|found| found.account.as_str());
                assert_eq!(spelled, Some("carol"), "{input}");
                assert_eq!(found, Some(carol()), "{input}");
            }
        }

        #[tokio::test]
        async fn unconfirmed_removed_and_unknown_members_are_not_found_or_issued_a_code() {
            let (db, _dir) = with_unverified_members().await;
            execute(
                &db,
                "INSERT INTO members (account, email, email_verified_at, joined_at)
                 VALUES ('dave', 'dave@example.org', 1, 0)",
            )
            .await
            .unwrap();
            db.remove_member(&MemberName::new("dave"), MAILED)
                .await
                .unwrap();
            for input in [
                "alice",
                "alice@example.org",
                "dave",
                "dave@example.org",
                "nobody",
                "nobody@example.org",
                "carol+irc@example.org",
            ] {
                let lookup = Lookup::parse(input).unwrap();
                assert_eq!(
                    db.recoverable_member(&lookup).await.unwrap(),
                    None,
                    "{input}"
                );
            }
            for account in ["alice", "dave", "nobody"] {
                let name = MemberName::new(account);
                let issued = db.issue_reset_code(&name, MAILED).await.unwrap();
                assert!(issued.is_none(), "{account}");
            }
        }

        #[tokio::test]
        async fn a_code_goes_to_the_confirmed_address_and_only_its_hash_is_kept() {
            let (db, dir) = with_unverified_members().await;

            let issued = issue(&db, "CAROL", MAILED).await;

            assert_eq!(issued.member, carol());
            assert_eq!(issued.code.expose().len(), link_code::MAILED_LENGTH);
            assert_eq!(issued.expires_at, reset_rules::expires_at(MAILED));
            assert!(!files_contain(&dir, issued.code.expose().as_bytes()));
        }

        #[tokio::test]
        async fn a_code_works_once_for_its_member() {
            let (db, _dir) = with_unverified_members().await;
            let issued = issue(&db, "carol", MAILED).await;

            let taken = db
                .take_reset_code(issued.code.expose(), MAILED)
                .await
                .unwrap();

            let taken = taken.unwrap();
            assert_eq!(
                (taken.account.as_str(), taken.email.as_str()),
                ("carol", "carol@example.org")
            );
            assert_eq!(
                db.take_reset_code(issued.code.expose(), MAILED)
                    .await
                    .unwrap(),
                None
            );
        }

        async fn reset_codes(db: &Db) -> i64 {
            sqlx::query_scalar("SELECT count(*) FROM password_reset_codes")
                .fetch_one(&db.pool)
                .await
                .unwrap()
        }

        #[tokio::test]
        async fn a_new_code_replaces_the_last() {
            let (db, _dir) = with_unverified_members().await;
            let first = issue(&db, "carol", MAILED).await;

            let second = issue(&db, "carol", MAILED + 60).await;

            assert_eq!(reset_codes(&db).await, 1);
            assert_eq!(
                db.take_reset_code(first.code.expose(), MAILED + 60)
                    .await
                    .unwrap(),
                None
            );
            assert!(
                db.take_reset_code(second.code.expose(), MAILED + 60)
                    .await
                    .unwrap()
                    .is_some()
            );
        }

        #[tokio::test]
        async fn a_code_works_until_it_expires_and_expired_ones_are_cleaned_up() {
            let (db, _dir) = with_unverified_members().await;
            execute(
                &db,
                "UPDATE members SET email_verified_at = 1 WHERE account = 'bob'",
            )
            .await
            .unwrap();
            let carols = issue(&db, "carol", MAILED).await;
            let bobs = issue(&db, "bob", MAILED).await;

            let just_before = db
                .take_reset_code(carols.code.expose(), carols.expires_at - 1)
                .await;
            let at_expiry = db
                .take_reset_code(bobs.code.expose(), bobs.expires_at)
                .await;

            assert!(just_before.unwrap().is_some());
            assert_eq!(at_expiry.unwrap(), None);
            issue(&db, "bob", MAILED).await;
            issue(&db, "carol", reset_rules::expires_at(MAILED)).await;
            assert_eq!(reset_codes(&db).await, 1);
        }

        #[tokio::test]
        async fn a_code_stops_working_once_its_address_changes_or_its_member_goes() {
            let (db, _dir) = with_unverified_members().await;
            let changed = issue(&db, "carol", MAILED).await;
            execute(
                &db,
                "UPDATE members SET email = 'carol@example.net' WHERE account = 'carol'",
            )
            .await
            .unwrap();
            assert_eq!(
                db.take_reset_code(changed.code.expose(), MAILED)
                    .await
                    .unwrap(),
                None
            );

            db.update_settings(
                &MemberName::new("carol"),
                &email("carol@example.org"),
                false,
            )
            .await
            .unwrap();
            execute(
                &db,
                "UPDATE members SET email_verified_at = 1 WHERE account = 'carol'",
            )
            .await
            .unwrap();
            let saved = issue(&db, "carol", MAILED).await;
            db.update_settings(
                &MemberName::new("carol"),
                &email("carol@example.net"),
                false,
            )
            .await
            .unwrap();
            db.update_settings(
                &MemberName::new("carol"),
                &email("carol@example.org"),
                false,
            )
            .await
            .unwrap();
            execute(
                &db,
                "UPDATE members SET email_verified_at = 1 WHERE account = 'carol'",
            )
            .await
            .unwrap();
            assert_eq!(
                db.take_reset_code(saved.code.expose(), MAILED)
                    .await
                    .unwrap(),
                None
            );

            let removed = issue(&db, "carol", MAILED).await;
            db.remove_member(&MemberName::new("carol"), MAILED)
                .await
                .unwrap();
            assert_eq!(
                db.take_reset_code(removed.code.expose(), MAILED)
                    .await
                    .unwrap(),
                None
            );
        }

        #[tokio::test]
        async fn a_taken_code_can_be_put_back_unless_a_newer_one_replaced_it() {
            let (db, _dir) = with_unverified_members().await;
            let issued = issue(&db, "carol", MAILED).await;
            let taken = db
                .take_reset_code(issued.code.expose(), MAILED)
                .await
                .unwrap()
                .unwrap();

            db.restore_reset_code(&taken).await.unwrap();

            let again = db
                .take_reset_code(issued.code.expose(), MAILED)
                .await
                .unwrap();
            assert_eq!(again.as_ref(), Some(&taken));
            let newer = issue(&db, "carol", MAILED + 60).await;
            db.restore_reset_code(&taken).await.unwrap();
            assert_eq!(
                db.take_reset_code(issued.code.expose(), MAILED)
                    .await
                    .unwrap(),
                None
            );
            assert!(
                db.take_reset_code(newer.code.expose(), MAILED)
                    .await
                    .unwrap()
                    .is_some()
            );
        }
    }

    /// RFC 7636, appendix B.
    static CHALLENGE: LazyLock<CodeChallenge> = LazyLock::new(|| {
        CodeChallenge::parse("E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM").unwrap()
    });

    fn hash<K>(bytes: &[u8]) -> SecretHash<K> {
        SecretHash::from_bytes(bytes)
    }

    fn code<'a>(
        session_hash: &'a SessionHash,
        code_hash: &'a AuthorizationCodeHash,
        expires_at: i64,
    ) -> NewAuthorizationCode<'a> {
        NewAuthorizationCode {
            code_hash,
            session_hash,
            client_id: "gamja",
            redirect_uri: "https://irc.weblounge.org/",
            code_challenge: &CHALLENGE,
            expires_at,
        }
    }

    fn token<'a>(
        session_hash: &'a SessionHash,
        token_hash: &'a AccessTokenHash,
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
        db.insert_authorization_code(&code(&hash(b"\xa1"), &hash(b"\xc1"), 60), 0)
            .await
            .unwrap();
        let taken = db
            .take_authorization_code(&hash(b"\xc1"))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            taken,
            AuthorizationCode {
                session_hash: hash(b"\xa1"),
                client_id: "gamja".into(),
                redirect_uri: "https://irc.weblounge.org/".into(),
                code_challenge: CHALLENGE.clone(),
                expires_at: 60,
            }
        );
        assert_eq!(
            db.take_authorization_code(&hash(b"\xc1")).await.unwrap(),
            None
        );
        assert_eq!(
            db.take_authorization_code(&hash(b"\xff")).await.unwrap(),
            None
        );
    }

    #[tokio::test]
    async fn a_code_with_a_malformed_challenge_is_taken_but_not_handed_back() {
        let (db, _dir) = with_sessions().await;
        db.insert_authorization_code(&code(&hash(b"\xa1"), &hash(b"\xc1"), 60), 0)
            .await
            .unwrap();
        sqlx::query("UPDATE oauth_codes SET code_challenge = 'plain'")
            .execute(&db.pool)
            .await
            .unwrap();
        assert_eq!(
            db.take_authorization_code(&hash(b"\xc1")).await.unwrap(),
            None
        );
        let left: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM oauth_codes")
            .fetch_one(&db.pool)
            .await
            .unwrap();
        assert_eq!(left, 0);
    }

    #[tokio::test]
    async fn expired_codes_are_cleaned_up_when_another_is_issued() {
        let (db, _dir) = with_sessions().await;
        db.insert_authorization_code(&code(&hash(b"\xa1"), &hash(b"\xc1"), 60), 0)
            .await
            .unwrap();
        db.insert_authorization_code(&code(&hash(b"\xa1"), &hash(b"\xc2"), 120), 60)
            .await
            .unwrap();
        assert_eq!(
            db.take_authorization_code(&hash(b"\xc1")).await.unwrap(),
            None
        );
        assert!(
            db.take_authorization_code(&hash(b"\xc2"))
                .await
                .unwrap()
                .is_some()
        );
    }

    #[tokio::test]
    async fn an_access_token_names_its_member_until_it_expires() {
        let (db, _dir) = with_sessions().await;
        db.insert_access_token(&token(&hash(b"\xa2"), &hash(b"\xd1"), 150))
            .await
            .unwrap();
        let live = db.access_token(&hash(b"\xd1"), 149).await.unwrap();
        assert_eq!(
            live,
            Some(AccessToken {
                account: MemberName::new("alice"),
                client_id: "gamja".into(),
                expires_at: 150,
            })
        );
        assert_eq!(db.access_token(&hash(b"\xd1"), 150).await.unwrap(), None);
        assert_eq!(db.access_token(&hash(b"\xff"), 0).await.unwrap(), None);
    }

    #[tokio::test]
    async fn codes_and_tokens_end_with_their_session() {
        let (db, _dir) = with_sessions().await;
        db.insert_authorization_code(&code(&hash(b"\xa1"), &hash(b"\xc1"), 60), 0)
            .await
            .unwrap();
        db.insert_access_token(&token(&hash(b"\xa1"), &hash(b"\xd1"), 300))
            .await
            .unwrap();
        db.insert_access_token(&token(&hash(b"\xa2"), &hash(b"\xd2"), 300))
            .await
            .unwrap();
        assert_eq!(
            db.access_token(&hash(b"\xd1"), 100).await.unwrap(),
            None,
            "a1 expired"
        );
        db.end_session(&hash(b"\xa1")).await.unwrap();
        assert_eq!(
            db.take_authorization_code(&hash(b"\xc1")).await.unwrap(),
            None
        );
        assert!(db.access_token(&hash(b"\xd2"), 0).await.unwrap().is_some());
        db.end_sessions(&MemberName::new("alice")).await.unwrap();
        assert_eq!(db.access_token(&hash(b"\xd2"), 0).await.unwrap(), None);
    }

    #[tokio::test]
    async fn a_token_needs_an_existing_session() {
        let (db, _dir) = with_sessions().await;
        assert!(
            db.insert_access_token(&token(&hash(b"\xee"), &hash(b"\xd1"), 300))
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn a_removed_members_tokens_name_nobody() {
        let (db, _dir) = with_sessions().await;
        db.insert_access_token(&token(&hash(b"\xa2"), &hash(b"\xd1"), 300))
            .await
            .unwrap();
        execute(
            &db,
            "UPDATE members SET email = NULL, removed_at = 1 WHERE account = 'alice'",
        )
        .await
        .unwrap();
        assert_eq!(db.access_token(&hash(b"\xd1"), 0).await.unwrap(), None);
    }

    /// luiz runs bender and dalek, alice runs Marvin; dalek was deleted.
    async fn with_bots() -> (Db, tempfile::TempDir) {
        let (db, dir) = temp_db().await;
        execute(
            &db,
            "INSERT INTO members (account, email, joined_at)
             VALUES ('luiz', 'luiz@example.org', 0), ('alice', 'alice@example.org', 0)",
        )
        .await
        .unwrap();
        for (account, owner, note) in [
            ("bender", "luiz", Some("answers !ping")),
            ("dalek", "luiz", None),
            ("Marvin", "alice", Some("posts the weather")),
        ] {
            let bot = NewBot {
                account: &BotName::new(account),
                owner: &MemberName::new(owner),
                note,
                created_at: 100,
            };
            let inserted = db.insert_bot(&bot, None, None).await.unwrap();
            assert_eq!(inserted, BotInsertion::Inserted);
        }
        assert!(db.remove_bot(&BotName::new("DALEK"), 200).await.unwrap());
        (db, dir)
    }

    #[tokio::test]
    async fn a_bot_is_found_by_its_name_in_any_case() {
        let (db, _dir) = with_bots().await;
        let bender = db.bot("BENDER").await.unwrap();
        assert_eq!(
            bender,
            Some(Bot {
                account: BotName::new("bender"),
                owner: MemberName::new("luiz"),
                note: Some("answers !ping".into()),
                created_at: 100,
                removed_at: None,
            })
        );
        assert_eq!(bender.unwrap().account.as_str(), "bender");
        assert_eq!(
            db.bot("dalek").await.unwrap().unwrap().removed_at,
            Some(200)
        );
        assert_eq!(db.bot("luiz").await.unwrap(), None);
    }

    #[tokio::test]
    async fn lists_the_live_bots_of_one_owner_or_of_everyone() {
        let (db, _dir) = with_bots().await;
        let names = |bots: Vec<Bot>| -> Vec<String> {
            bots.into_iter()
                .map(|bot| bot.account.to_string())
                .collect()
        };
        let owner = MemberName::new;
        assert_eq!(names(db.bots(None).await.unwrap()), ["bender", "Marvin"]);
        assert_eq!(
            names(db.bots(Some(&owner("LUIZ"))).await.unwrap()),
            ["bender"]
        );
        assert!(db.bots(Some(&owner("nobody"))).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn inserts_a_bot_only_while_its_owner_has_room_for_it() {
        let (db, _dir) = with_bots().await;
        let owner = MemberName::new("LUIZ");
        let bot = |account| NewBot {
            account,
            owner: &owner,
            note: None,
            created_at: 300,
        };

        let names = [BotName::new("hal"), BotName::new("eve")];
        let (hal, eve) = (bot(&names[0]), bot(&names[1]));
        let inserted = db.insert_bot(&hal, None, Some(2)).await.unwrap();
        assert_eq!(inserted, BotInsertion::Inserted);
        let inserted = db.insert_bot(&eve, None, Some(2)).await.unwrap();
        assert_eq!(inserted, BotInsertion::TooMany);

        assert_eq!(db.bot("eve").await.unwrap(), None);
        assert!(db.bot("hal").await.unwrap().is_some());
        let inserted = db.insert_bot(&eve, None, Some(3)).await.unwrap();
        assert_eq!(inserted, BotInsertion::Inserted);
    }

    #[tokio::test]
    async fn two_bots_at_once_cannot_both_take_the_last_place() {
        let (db, _dir) = with_bots().await;
        let owner = MemberName::new("luiz");
        let bot = |account| NewBot {
            account,
            owner: &owner,
            note: None,
            created_at: 300,
        };

        let names = [BotName::new("hal"), BotName::new("eve")];
        let (hal, eve) = (bot(&names[0]), bot(&names[1]));
        let inserted = tokio::join!(
            db.insert_bot(&hal, None, Some(2)),
            db.insert_bot(&eve, None, Some(2))
        );

        let inserted = [inserted.0.unwrap(), inserted.1.unwrap()];
        assert!(inserted.contains(&BotInsertion::Inserted), "{inserted:?}");
        assert!(inserted.contains(&BotInsertion::TooMany), "{inserted:?}");
        assert_eq!(db.bots(Some(&owner)).await.unwrap().len(), 2);
    }

    #[tokio::test]
    async fn lists_every_bot_with_whether_it_or_its_owner_is_gone() {
        let (db, _dir) = with_bots().await;
        execute(
            &db,
            "UPDATE members SET email = NULL, removed_at = 300 WHERE account = 'alice'",
        )
        .await
        .unwrap();
        let accounts = db.bot_accounts().await.unwrap();
        let accounts: Vec<_> = accounts
            .iter()
            .map(|bot| {
                let names = (bot.account.as_str(), bot.owner.as_str());
                (names, bot.removed, bot.owner_removed)
            })
            .collect();
        assert_eq!(
            accounts,
            [
                (("bender", "luiz"), false, false),
                (("dalek", "luiz"), true, false),
                (("Marvin", "alice"), false, true),
            ]
        );
    }

    #[tokio::test]
    async fn a_bot_is_removed_once_and_needs_an_owner_and_a_free_name() {
        let (db, _dir) = with_bots().await;
        assert!(!db.remove_bot(&BotName::new("dalek"), 300).await.unwrap());
        assert!(!db.remove_bot(&BotName::new("nobody"), 300).await.unwrap());
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
        let hal = (BotName::new("hal"), MemberName::new("nobody"));
        let error = db
            .insert_bot(&bot(&hal.0, &hal.1), None, None)
            .await
            .unwrap_err();
        assert!(error.to_string().contains("FOREIGN KEY"), "{error}");
        let dalek = (BotName::new("Dalek"), MemberName::new("alice"));
        let inserted = db.insert_bot(&bot(&dalek.0, &dalek.1), None, None).await;
        assert_eq!(inserted.unwrap(), BotInsertion::NameTaken);
    }

    #[tokio::test]
    async fn two_members_at_once_cannot_both_reuse_a_deleted_bots_name() {
        let (db, _dir) = with_bots().await;
        let dalek = db.bot("dalek").await.unwrap().unwrap();
        let name = BotName::new("dalek");
        let bot = |owner| NewBot {
            account: &name,
            owner,
            note: None,
            created_at: 300,
        };
        let owners = [MemberName::new("luiz"), MemberName::new("alice")];
        let (luiz, alice) = (bot(&owners[0]), bot(&owners[1]));

        let inserted = tokio::join!(
            db.insert_bot(&luiz, Some(&dalek), None),
            db.insert_bot(&alice, Some(&dalek), None)
        );

        let inserted = [inserted.0.unwrap(), inserted.1.unwrap()];
        assert!(inserted.contains(&BotInsertion::Inserted), "{inserted:?}");
        assert!(inserted.contains(&BotInsertion::NameTaken), "{inserted:?}");
        let live = db.bots(None).await.unwrap();
        assert_eq!(live.iter().filter(|bot| bot.account == name).count(), 1);
    }

    #[tokio::test]
    async fn a_new_bot_takes_the_row_of_the_deleted_bot_it_was_judged_against() {
        let (db, _dir) = with_bots().await;
        let dalek = db.bot("dalek").await.unwrap().unwrap();
        let bender = db.bot("bender").await.unwrap().unwrap();
        let (account, alice) = (BotName::new("Dalek"), MemberName::new("alice"));
        let hal = NewBot {
            account: &account,
            owner: &alice,
            note: Some("takes over"),
            created_at: 300,
        };
        let changed_since = Bot {
            removed_at: Some(199),
            ..dalek.clone()
        };
        for stale in [&changed_since, &bender] {
            let inserted = db.insert_bot(&hal, Some(stale), None).await.unwrap();
            assert_eq!(inserted, BotInsertion::NameTaken, "{stale:?}");
        }
        assert_eq!(db.bot("dalek").await.unwrap().as_ref(), Some(&dalek));

        let inserted = db.insert_bot(&hal, Some(&dalek), None).await.unwrap();

        assert_eq!(inserted, BotInsertion::Inserted);
        let taken_over = db.bot("DALEK").await.unwrap();
        assert_eq!(
            taken_over,
            Some(Bot {
                account: BotName::new("dalek"),
                owner: alice.clone(),
                note: Some("takes over".into()),
                created_at: 300,
                removed_at: None,
            })
        );
        assert_eq!(taken_over.unwrap().account.as_str(), "dalek");
        assert_eq!(db.bots(Some(&alice)).await.unwrap().len(), 2);
    }

    #[tokio::test]
    async fn taking_back_a_bot_frees_its_name_or_puts_back_the_one_it_replaced() {
        let (db, _dir) = with_bots().await;
        let dalek = db.bot("dalek").await.unwrap().unwrap();
        let owner = MemberName::new("alice");
        let bot = |account| NewBot {
            account,
            owner: &owner,
            note: None,
            created_at: 300,
        };
        let (hal, new_dalek) = (BotName::new("hal"), BotName::new("dalek"));
        db.insert_bot(&bot(&hal), None, None).await.unwrap();
        db.insert_bot(&bot(&new_dalek), Some(&dalek), None)
            .await
            .unwrap();

        db.take_back_bot(&BotName::new("HAL"), None).await.unwrap();
        db.take_back_bot(&new_dalek, Some(&dalek)).await.unwrap();

        assert_eq!(db.bot("hal").await.unwrap(), None);
        assert_eq!(db.bot("dalek").await.unwrap(), Some(dalek));
    }

    #[tokio::test]
    async fn members_hold_their_names_in_any_case_even_once_removed() {
        let (db, _dir) = with_bots().await;
        execute(
            &db,
            "INSERT INTO members (account, joined_at, removed_at)
             VALUES ('Gone', 0, 1)",
        )
        .await
        .unwrap();
        for name in ["luiz", "ALICE", "gone"] {
            assert!(db.held_by_member(name).await.unwrap(), "{name}");
        }
        for name in ["bob", "luiz_", "Bender", "dalek"] {
            assert!(!db.held_by_member(name).await.unwrap(), "{name}");
        }
    }

    #[tokio::test]
    async fn a_removed_members_session_signs_nobody_in() {
        let (db, _dir) = with_sessions().await;
        execute(
            &db,
            "UPDATE members SET email = NULL, removed_at = 1 WHERE account = 'alice'",
        )
        .await
        .unwrap();
        assert_eq!(db.session(&hash(b"\xa2"), 0).await.unwrap(), None);
    }
}
