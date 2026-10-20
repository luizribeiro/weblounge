//! The uploads table: whose each upload is and how big, for the Uploads
//! tab, quotas, expiry and deletion. The files are the scrubber's.

use filehost::{ImageType, UploadId};

use super::Db;
use crate::name::{BotName, MemberName};

pub struct NewUpload<'a> {
    pub id: &'a UploadId,
    pub kind: ImageType,
    /// The member it counts against.
    pub owner: &'a MemberName,
    /// The owner's bot that uploaded it, or None for the owner.
    pub bot: Option<&'a BotName>,
    pub size: i64,
    pub created_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Upload {
    pub id: UploadId,
    pub kind: ImageType,
    pub owner: MemberName,
    pub bot: Option<BotName>,
    pub size: i64,
    pub created_at: i64,
}

impl Upload {
    pub fn file_name(&self) -> String {
        self.id.file_name(self.kind)
    }
}

struct Row {
    id: UploadId,
    account: String,
    owner: MemberName,
    content_type: String,
    size: i64,
    created_at: i64,
}

impl Row {
    /// The upload, unless its content type is none the network takes,
    /// which no row of ours has.
    fn upload(self) -> Option<Upload> {
        let by_owner = self.account.eq_ignore_ascii_case(self.owner.as_str());
        Some(Upload {
            id: self.id,
            kind: ImageType::from_content_type(&self.content_type)?,
            bot: (!by_owner).then(|| BotName::new(self.account)),
            owner: self.owner,
            size: self.size,
            created_at: self.created_at,
        })
    }
}

fn uploads(rows: Vec<Row>) -> Vec<Upload> {
    rows.into_iter().filter_map(Row::upload).collect()
}

impl Db {
    /// Inserts `upload` unless its owner's uploads would then take more
    /// than `quota` bytes; answers whether it did. Concurrent calls take
    /// turns, so two uploads can't both take the last of the room.
    pub async fn insert_upload(&self, upload: &NewUpload<'_>, quota: i64) -> sqlx::Result<bool> {
        let mut transaction = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let used = upload_usage(&mut *transaction, upload.owner).await?;
        if used + upload.size > quota {
            return Ok(false);
        }
        let account = upload.bot.map_or(upload.owner.as_str(), BotName::as_str);
        let content_type = upload.kind.content_type();
        sqlx::query!(
            "INSERT INTO uploads (id, account, owner, content_type, size, created_at)
             VALUES (?, ?, ?, ?, ?, ?)",
            upload.id,
            account,
            upload.owner,
            content_type,
            upload.size,
            upload.created_at,
        )
        .execute(&mut *transaction)
        .await?;
        transaction.commit().await?;
        Ok(true)
    }

    /// The bytes `owner`'s uploads take, their bots' included.
    pub async fn upload_usage(&self, owner: &MemberName) -> sqlx::Result<i64> {
        upload_usage(&self.pool, owner).await
    }

    /// `owner`'s uploads, their bots' included, newest first.
    pub async fn uploads_of(&self, owner: &MemberName) -> sqlx::Result<Vec<Upload>> {
        let rows = sqlx::query_as!(
            Row,
            r#"SELECT id AS "id: UploadId", account, owner AS "owner: MemberName",
                      content_type, size, created_at
               FROM uploads WHERE owner = ? ORDER BY created_at DESC, id"#,
            owner
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(uploads(rows))
    }

    /// The uploads `bot` made.
    pub async fn uploads_by(&self, bot: &BotName) -> sqlx::Result<Vec<Upload>> {
        let rows = sqlx::query_as!(
            Row,
            r#"SELECT id AS "id: UploadId", account, owner AS "owner: MemberName",
                      content_type, size, created_at
               FROM uploads WHERE account = ? AND account != owner"#,
            bot
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(uploads(rows))
    }

    /// The uploads made at or before `at`.
    pub async fn uploads_made_by(&self, at: i64) -> sqlx::Result<Vec<Upload>> {
        let rows = sqlx::query_as!(
            Row,
            r#"SELECT id AS "id: UploadId", account, owner AS "owner: MemberName",
                      content_type, size, created_at
               FROM uploads WHERE created_at <= ?"#,
            at
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(uploads(rows))
    }

    pub async fn upload(&self, id: &UploadId) -> sqlx::Result<Option<Upload>> {
        let row = sqlx::query_as!(
            Row,
            r#"SELECT id AS "id: UploadId", account, owner AS "owner: MemberName",
                      content_type, size, created_at
               FROM uploads WHERE id = ?"#,
            id
        )
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.and_then(Row::upload))
    }

    /// Every upload's id, to tell the files that have a row from those
    /// that don't.
    pub async fn upload_ids(&self) -> sqlx::Result<Vec<UploadId>> {
        sqlx::query_scalar!(r#"SELECT id AS "id: UploadId" FROM uploads"#)
            .fetch_all(&self.pool)
            .await
    }

    /// Answers whether there was such an upload.
    pub async fn delete_upload(&self, id: &UploadId) -> sqlx::Result<bool> {
        let deleted = sqlx::query!("DELETE FROM uploads WHERE id = ?", id)
            .execute(&self.pool)
            .await?;
        Ok(deleted.rows_affected() == 1)
    }
}

async fn upload_usage(
    executor: impl sqlx::SqliteExecutor<'_>,
    owner: &MemberName,
) -> sqlx::Result<i64> {
    sqlx::query_scalar!(
        r#"SELECT COALESCE(SUM(size), 0) AS "used!: i64" FROM uploads WHERE owner = ?"#,
        owner
    )
    .fetch_one(executor)
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::tests::{execute, temp_db};

    const ALICES: &str = "q7Hx2mZp9vLcT4wRfBa-_w";
    const MARVINS: &str = "Zt4wRfBa-_wq7Hx2mZp9vL";
    const LUIZS: &str = "9vLcT4wRfBa-_wq7Hx2mZp";

    fn id(id: &str) -> UploadId {
        UploadId::parse(id).unwrap()
    }

    /// alice uploaded a photo at 100 and her bot Marvin a GIF at 200; luiz
    /// a screenshot at 300.
    async fn with_uploads() -> (Db, tempfile::TempDir) {
        let (db, dir) = temp_db().await;
        execute(
            &db,
            "INSERT INTO members (account, email, joined_at)
             VALUES ('luiz', 'luiz@example.org', 0), ('alice', 'alice@example.org', 0);
             INSERT INTO bots (account, owner, created_at) VALUES ('Marvin', 'alice', 0)",
        )
        .await
        .unwrap();
        let (alice, luiz, marvin) = (
            MemberName::new("alice"),
            MemberName::new("luiz"),
            BotName::new("Marvin"),
        );
        for (upload, kind, owner, bot, size, created_at) in [
            (ALICES, ImageType::Jpeg, &alice, None, 2_000_000, 100),
            (MARVINS, ImageType::Gif, &alice, Some(&marvin), 500_000, 200),
            (LUIZS, ImageType::Png, &luiz, None, 80_000, 300),
        ] {
            let upload = NewUpload {
                id: &id(upload),
                kind,
                owner,
                bot,
                size,
                created_at,
            };
            assert!(db.insert_upload(&upload, i64::MAX).await.unwrap());
        }
        (db, dir)
    }

    fn ids(uploads: Vec<Upload>) -> Vec<String> {
        let mut ids: Vec<_> = uploads.iter().map(|u| u.id.as_str().to_owned()).collect();
        ids.sort();
        ids
    }

    #[tokio::test]
    async fn a_member_sees_their_own_uploads_and_their_bots_newest_first() {
        let (db, _dir) = with_uploads().await;

        let alices = db.uploads_of(&MemberName::new("ALICE")).await.unwrap();

        assert_eq!(
            alices,
            [
                Upload {
                    id: id(MARVINS),
                    kind: ImageType::Gif,
                    owner: MemberName::new("alice"),
                    bot: Some(BotName::new("Marvin")),
                    size: 500_000,
                    created_at: 200,
                },
                Upload {
                    id: id(ALICES),
                    kind: ImageType::Jpeg,
                    owner: MemberName::new("alice"),
                    bot: None,
                    size: 2_000_000,
                    created_at: 100,
                },
            ]
        );
        assert_eq!(alices[0].file_name(), format!("{MARVINS}.gif"));
        assert_eq!(db.upload(&id(LUIZS)).await.unwrap().unwrap().bot, None);
    }

    #[tokio::test]
    async fn counts_a_bots_uploads_against_its_owner() {
        let (db, _dir) = with_uploads().await;
        for (owner, used) in [("alice", 2_500_000), ("luiz", 80_000), ("bob", 0)] {
            let owner = MemberName::new(owner);
            assert_eq!(db.upload_usage(&owner).await.unwrap(), used, "{owner}");
        }
    }

    #[tokio::test]
    async fn inserts_an_upload_only_while_its_owner_has_room_for_it() {
        let (db, _dir) = with_uploads().await;
        let (alice, marvin) = (MemberName::new("alice"), BotName::new("marvin"));
        let ids = [id("AAAAAAAAAAAAAAAAAAAAAA"), id("BBBBBBBBBBBBBBBBBBBBBB")];
        let upload = |id, size| NewUpload {
            id,
            kind: ImageType::Webp,
            owner: &alice,
            bot: Some(&marvin),
            size,
            created_at: 400,
        };
        let quota = 3_000_000;

        let too_big = upload(&ids[0], 500_001);
        assert!(!db.insert_upload(&too_big, quota).await.unwrap());
        let fits = upload(&ids[1], 500_000);
        assert!(db.insert_upload(&fits, quota).await.unwrap());

        assert_eq!(db.upload(too_big.id).await.unwrap(), None);
        assert_eq!(db.upload_usage(&alice).await.unwrap(), quota);
    }

    #[tokio::test]
    async fn two_uploads_at_once_cannot_both_take_the_last_of_the_room() {
        let (db, _dir) = with_uploads().await;
        let alice = MemberName::new("alice");
        let (first, second) = (id("AAAAAAAAAAAAAAAAAAAAAA"), id("BBBBBBBBBBBBBBBBBBBBBB"));
        let upload = |id| NewUpload {
            id,
            kind: ImageType::Png,
            owner: &alice,
            bot: None,
            size: 400_000,
            created_at: 400,
        };
        let (first, second) = (upload(&first), upload(&second));

        let inserted = tokio::join!(
            db.insert_upload(&first, 3_000_000),
            db.insert_upload(&second, 3_000_000)
        );

        let inserted = [inserted.0.unwrap(), inserted.1.unwrap()];
        assert_eq!(inserted.iter().filter(|&&inserted| inserted).count(), 1);
        assert_eq!(db.upload_usage(&alice).await.unwrap(), 2_900_000);
    }

    #[tokio::test]
    async fn finds_a_bots_uploads_and_those_past_an_age() {
        let (db, _dir) = with_uploads().await;
        assert_eq!(
            ids(db.uploads_by(&BotName::new("MARVIN")).await.unwrap()),
            [MARVINS]
        );
        assert!(
            db.uploads_by(&BotName::new("alice"))
                .await
                .unwrap()
                .is_empty()
        );
        assert_eq!(ids(db.uploads_made_by(199).await.unwrap()), [ALICES]);
        let mut all = vec![ALICES, MARVINS, LUIZS];
        all.sort();
        assert_eq!(ids(db.uploads_made_by(300).await.unwrap()), all);
        let mut every_id: Vec<_> = db.upload_ids().await.unwrap();
        every_id.sort_by(|a, b| a.as_str().cmp(b.as_str()));
        assert_eq!(
            every_id.iter().map(UploadId::as_str).collect::<Vec<_>>(),
            all
        );
    }

    #[tokio::test]
    async fn deletes_an_upload_once() {
        let (db, _dir) = with_uploads().await;
        assert!(db.delete_upload(&id(MARVINS)).await.unwrap());
        assert!(!db.delete_upload(&id(MARVINS)).await.unwrap());
        assert_eq!(
            ids(db.uploads_of(&MemberName::new("alice")).await.unwrap()),
            [ALICES]
        );
    }
}
