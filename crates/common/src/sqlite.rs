//! What concierge's database and the outbox share: how they're opened, how a
//! service checks it's opening one at its build's schema, and how deleted
//! rows leave the disk.

use std::{path::Path, time::Duration};

use anyhow::Context;
use sqlx::{
    SqlitePool,
    migrate::Migrator,
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous},
};

const BUSY_TIMEOUT: Duration = Duration::from_secs(5);

#[doc(hidden)]
pub use sqlx;

/// Stores the newtype `$type`, generic over one kind or not, in SQLite as
/// its field `$field` of type `$inner`: sqlx binds it as one, and given
/// `$new`, which makes a `$type` of an `$inner`, reads it back as one too.
#[macro_export]
macro_rules! sqlite_newtype {
    ($type:ident $(<$kind:ident>)?, $field:tt: $inner:ty) => {
        impl $(<$kind>)? $crate::sqlite::sqlx::Type<$crate::sqlite::sqlx::Sqlite>
            for $type $(<$kind>)?
        {
            fn type_info() -> $crate::sqlite::sqlx::sqlite::SqliteTypeInfo {
                <$inner as $crate::sqlite::sqlx::Type<$crate::sqlite::sqlx::Sqlite>>::type_info()
            }

            fn compatible(ty: &$crate::sqlite::sqlx::sqlite::SqliteTypeInfo) -> bool {
                <$inner as $crate::sqlite::sqlx::Type<$crate::sqlite::sqlx::Sqlite>>::compatible(
                    ty,
                )
            }
        }

        impl<'q $(, $kind)?> $crate::sqlite::sqlx::Encode<'q, $crate::sqlite::sqlx::Sqlite>
            for $type $(<$kind>)?
        {
            fn encode_by_ref(
                &self,
                args: &mut Vec<$crate::sqlite::sqlx::sqlite::SqliteArgumentValue<'q>>,
            ) -> Result<$crate::sqlite::sqlx::encode::IsNull, $crate::sqlite::sqlx::error::BoxDynError>
            {
                <$inner as $crate::sqlite::sqlx::Encode<'q, $crate::sqlite::sqlx::Sqlite>>::encode_by_ref(
                    &self.$field,
                    args,
                )
            }
        }
    };
    ($type:ident $(<$kind:ident>)?, $field:tt: $inner:ty, $new:expr) => {
        $crate::sqlite_newtype!($type $(<$kind>)?, $field: $inner);

        impl<'r $(, $kind)?> $crate::sqlite::sqlx::Decode<'r, $crate::sqlite::sqlx::Sqlite>
            for $type $(<$kind>)?
        {
            fn decode(
                value: $crate::sqlite::sqlx::sqlite::SqliteValueRef<'r>,
            ) -> Result<Self, $crate::sqlite::sqlx::error::BoxDynError> {
                <$inner as $crate::sqlite::sqlx::Decode<'r, $crate::sqlite::sqlx::Sqlite>>::decode(
                    value,
                )
                .map($new)
            }
        }
    };
}

/// The database at `path`, which `create` creates if it's missing.
pub async fn connect(path: &Path, create: bool) -> anyhow::Result<SqlitePool> {
    let options = SqliteConnectOptions::new()
        .filename(path)
        .create_if_missing(create)
        .journal_mode(SqliteJournalMode::Wal)
        .synchronous(SqliteSynchronous::Normal)
        .foreign_keys(true)
        .pragma("secure_delete", "ON")
        .busy_timeout(BUSY_TIMEOUT);
    SqlitePoolOptions::new()
        .connect_with(options)
        .await
        .with_context(|| format!("can't open the database {}", path.display()))
}

/// The newest migration `migrator` knows.
pub fn latest_migration(migrator: &Migrator) -> i64 {
    migrator
        .iter()
        .map(|migration| migration.version)
        .max()
        .unwrap_or_default()
}

/// The newest migration applied to `pool`'s database, if any.
pub async fn migration_version(pool: &SqlitePool) -> sqlx::Result<Option<i64>> {
    sqlx::query_scalar("SELECT MAX(version) FROM _sqlx_migrations WHERE success")
        .fetch_one(pool)
        .await
}

/// Fails unless `concierge migrate` brought the database at `path` up to
/// `migrator`'s schema.
pub async fn ensure_migrated(
    pool: &SqlitePool,
    migrator: &Migrator,
    path: &Path,
) -> anyhow::Result<()> {
    let version = migration_version(pool)
        .await
        .with_context(|| format!("can't read the migrations of {}", path.display()))?;
    let latest = latest_migration(migrator);
    anyhow::ensure!(
        version == Some(latest),
        "the database {} is at migration {}, not {latest}; `concierge migrate` brings it up to date",
        path.display(),
        version.map_or_else(|| "none".to_owned(), |version| version.to_string()),
    );
    Ok(())
}

/// Copies the write-ahead log into the database and truncates it, so the
/// rows a change overwrote leave the disk, not just the database. Answers
/// whether it could, which a reader holding the log prevents.
pub async fn empty_write_ahead_log(pool: &SqlitePool) -> bool {
    let checkpoint: sqlx::Result<(i64, i64, i64)> =
        sqlx::query_as("PRAGMA wal_checkpoint(TRUNCATE)")
            .fetch_one(pool)
            .await;
    matches!(checkpoint, Ok((0, _, _)))
}

#[cfg(feature = "testing")]
pub mod testing {
    /// Whether any of the database files in `dir` holds `text`.
    pub fn files_contain(dir: &tempfile::TempDir, text: &[u8]) -> bool {
        let on_disk: Vec<u8> = std::fs::read_dir(dir.path())
            .unwrap()
            .flat_map(|entry| std::fs::read(entry.unwrap().path()).unwrap())
            .collect();
        on_disk.windows(text.len()).any(|window| window == text)
    }
}
