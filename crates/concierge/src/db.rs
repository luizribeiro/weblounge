use std::{path::Path, time::Duration};

use anyhow::Context;
use sqlx::{
    SqlitePool,
    migrate::Migrator,
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous},
};

static MIGRATOR: Migrator = sqlx::migrate!();
const BUSY_TIMEOUT: Duration = Duration::from_secs(5);

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
}
