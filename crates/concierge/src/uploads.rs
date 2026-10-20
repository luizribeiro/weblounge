//! Uploads from concierge's side. Concierge never reads what's uploaded:
//! it hands the bytes to the scrubber, a unit of its own with no network
//! that sees nothing but the uploads directory, which answers what it made
//! of them. Concierge only deletes the files the scrubber stored.

use std::{io, path::PathBuf, time::Duration};

use axum::body::Body;
use common::config::{ConfigError, required};
use filehost::{
    UploadId,
    handoff::{self, Answer},
};
use http_body_util::BodyExt;
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::UnixStream,
};

use crate::db::{Db, Upload};

/// How long the scrubber has to answer once it has the whole upload.
const ANSWER_WAIT: Duration = Duration::from_secs(30);

/// Where uploads are handed over and stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Store {
    /// The directory the scrubber stores uploads in.
    pub directory: PathBuf,
    /// The scrubber's socket.
    pub scrubber: PathBuf,
}

/// Why an upload went nowhere.
#[derive(Debug)]
pub enum HandOffError {
    /// The client sent more than [`filehost::MAX_BYTES`]; the scrubber
    /// was cut off and stored nothing.
    TooLarge,
    /// Reading the upload from the client failed.
    Client(axum::Error),
    /// The scrubber couldn't be reached, or didn't answer as it should.
    Scrubber(io::Error),
}

impl From<io::Error> for HandOffError {
    fn from(error: io::Error) -> Self {
        Self::Scrubber(error)
    }
}

impl Store {
    /// The store CONCIERGE_UPLOADS and CONCIERGE_SCRUBBER name.
    pub fn from_vars(var: impl Fn(&str) -> Option<String>) -> Result<Self, ConfigError> {
        Ok(Self {
            directory: required(&var, "CONCIERGE_UPLOADS")?.into(),
            scrubber: required(&var, "CONCIERGE_SCRUBBER")?.into(),
        })
    }

    /// Streams `body` to a scrubber, to store under `id` if it's an image,
    /// and answers what the scrubber made of it. A body past the limit
    /// is cut off as soon as it's past it.
    pub async fn hand_off(&self, id: &UploadId, mut body: Body) -> Result<Answer, HandOffError> {
        let mut scrubber = UnixStream::connect(&self.scrubber).await?;
        scrubber.write_all(handoff::head(id).as_bytes()).await?;
        let mut sent = 0;
        while let Some(frame) = body.frame().await {
            let Ok(chunk) = frame.map_err(HandOffError::Client)?.into_data() else {
                continue;
            };
            sent += chunk.len();
            if sent > filehost::MAX_BYTES {
                return Err(HandOffError::TooLarge);
            }
            if !chunk.is_empty() {
                scrubber
                    .write_all(&handoff::frame_header(chunk.len()))
                    .await?;
                scrubber.write_all(&chunk).await?;
            }
        }
        scrubber.write_all(&handoff::END).await?;
        let mut answer = String::new();
        let mut reader = BufReader::new(scrubber);
        tokio::time::timeout(ANSWER_WAIT, reader.read_line(&mut answer))
            .await
            .map_err(|_| io::Error::from(io::ErrorKind::TimedOut))??;
        Answer::parse(&answer)
            .ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidData, "the scrubber gave no answer")
            })
            .map_err(HandOffError::from)
    }

    /// Deletes `upload`'s file, unless it's gone already.
    pub fn remove_file(&self, upload: &Upload) -> io::Result<()> {
        match std::fs::remove_file(self.directory.join(upload.file_name())) {
            Err(error) if error.kind() != io::ErrorKind::NotFound => Err(error),
            _ => Ok(()),
        }
    }

    /// Deletes `uploads`, each file before its row: whatever deleting
    /// leaves behind if it stops partway still has a row, listed on its
    /// owner's Uploads tab to delete again.
    pub async fn delete(&self, db: &Db, uploads: &[Upload]) -> anyhow::Result<()> {
        for upload in uploads {
            self.remove_file(upload)?;
            db.delete_upload(&upload.id).await?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use filehost::{ImageType, testing::FakeScrubber};

    use super::*;
    use crate::{
        db::{NewUpload, tests::temp_db},
        name::MemberName,
    };

    const PNG: &[u8] = b"\x89PNG\r\n\x1a\nnot really a png";

    fn store(scrubber: &FakeScrubber) -> Store {
        Store {
            directory: scrubber.uploads.clone(),
            scrubber: scrubber.socket.clone(),
        }
    }

    /// A body of `chunks`, as a client streams it.
    fn streamed(chunks: Vec<Vec<u8>>) -> Body {
        let chunks = chunks.into_iter().map(Ok::<_, io::Error>);
        Body::from_stream(futures_util::stream::iter(chunks))
    }

    #[tokio::test]
    async fn hands_an_upload_to_the_scrubber_and_answers_what_it_made_of_it() {
        let scrubber = FakeScrubber::start();
        let id = UploadId::generate();

        let body = streamed(vec![PNG[..5].to_vec(), Vec::new(), PNG[5..].to_vec()]);
        let answer = store(&scrubber).hand_off(&id, body).await.unwrap();

        let size = PNG.len() as u64;
        assert_eq!(
            answer,
            Answer::Stored {
                kind: ImageType::Png,
                size
            }
        );
        assert_eq!(scrubber.stored(), [id.file_name(ImageType::Png)]);
        let stored = std::fs::read(scrubber.uploads.join(id.file_name(ImageType::Png)));
        assert_eq!(stored.unwrap(), PNG);
    }

    #[tokio::test]
    async fn passes_on_the_scrubbers_refusal() {
        let scrubber = FakeScrubber::start();
        let svg = Body::from(r#"<svg xmlns="http://www.w3.org/2000/svg"/>"#);

        let answer = store(&scrubber).hand_off(&UploadId::generate(), svg).await;

        assert_eq!(answer.unwrap(), Answer::NotAnImage);
        assert!(scrubber.stored().is_empty());
    }

    #[tokio::test]
    async fn cuts_off_an_upload_past_the_limit_and_nothing_is_stored() {
        let scrubber = FakeScrubber::start();
        let half = vec![0x89; filehost::MAX_BYTES / 2];
        let body = streamed(vec![half.clone(), half, vec![0]]);

        let answer = store(&scrubber).hand_off(&UploadId::generate(), body).await;

        assert!(matches!(answer, Err(HandOffError::TooLarge)), "{answer:?}");
        assert!(scrubber.stored().is_empty());
    }

    #[tokio::test]
    async fn fails_while_the_scrubber_is_down() {
        let scrubber = FakeScrubber::start();
        let store = Store {
            scrubber: scrubber.uploads.join("no-scrubber.sock"),
            ..store(&scrubber)
        };

        let answer = store.hand_off(&UploadId::generate(), Body::from(PNG)).await;

        assert!(
            matches!(answer, Err(HandOffError::Scrubber(_))),
            "{answer:?}"
        );
    }

    #[tokio::test]
    async fn deletes_files_and_rows_and_minds_no_file_already_gone() {
        let scrubber = FakeScrubber::start();
        let store = store(&scrubber);
        let (db, _dir) = temp_db().await;
        crate::db::tests::execute(
            &db,
            "INSERT INTO members (account, email, joined_at) VALUES ('alice', 'a@example.org', 0)",
        )
        .await
        .unwrap();
        let alice = MemberName::new("alice");
        let ids = [UploadId::generate(), UploadId::generate()];
        for id in &ids {
            let answer = store.hand_off(id, Body::from(PNG)).await.unwrap();
            assert!(matches!(answer, Answer::Stored { .. }));
            let upload = NewUpload {
                id,
                kind: ImageType::Png,
                owner: &alice,
                bot: None,
                size: PNG.len() as i64,
                created_at: 100,
            };
            db.insert_upload(&upload, i64::MAX).await.unwrap();
        }
        let uploads = db.uploads_of(&alice).await.unwrap();
        store.remove_file(&uploads[0]).unwrap();

        store.delete(&db, &uploads).await.unwrap();

        assert!(scrubber.stored().is_empty());
        assert!(db.uploads_of(&alice).await.unwrap().is_empty());
    }
}
