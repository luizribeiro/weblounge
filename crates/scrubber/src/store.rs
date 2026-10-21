//! Taking an upload from concierge: reading it, scrubbing it, and storing
//! what's left in the directory nginx serves, under its id and the
//! extension of the type it really is.

use std::{
    fs::Permissions,
    io::{Read, Write},
    os::unix::fs::PermissionsExt,
    path::Path,
};

use anyhow::Context;
use filehost::{
    UploadId,
    handoff::{self, Answer, ReadError},
};
use tempfile::NamedTempFile;

use crate::{Scrubbed, scrub};

/// Readable by the directory's group, which nginx is in.
const MODE: u32 = 0o640;

/// Reads an upload from `input`, stores it in `directory` if it's an
/// image, and answers on `output`. An upload cut off, or a request that
/// isn't one, gets no answer and stores nothing.
pub fn take(directory: &Path, input: impl Read, mut output: impl Write) -> anyhow::Result<Answer> {
    let answer = match handoff::read_upload(input) {
        Ok(upload) => match scrub(&upload.bytes) {
            Some(scrubbed) => {
                store(directory, &upload.id, &scrubbed)?;
                Answer::Stored {
                    kind: scrubbed.kind,
                    size: scrubbed.bytes.len() as u64,
                }
            }
            None => Answer::NotAnImage,
        },
        Err(ReadError::TooLarge) => Answer::TooLarge,
        Err(ReadError::CutOff) => anyhow::bail!("the upload was cut off"),
        Err(ReadError::Malformed) => anyhow::bail!("that isn't an upload"),
        Err(ReadError::Io(error)) => return Err(error).context("can't read the upload"),
    };
    output.write_all(answer.line().as_bytes())?;
    output.flush()?;
    Ok(answer)
}

/// Writes `scrubbed` under a temporary name, then moves it into place, so
/// a file is either whole or not there.
fn store(directory: &Path, id: &UploadId, scrubbed: &Scrubbed) -> anyhow::Result<()> {
    let path = directory.join(id.file_name(scrubbed.kind));
    let mut file = unserved_file(directory)
        .with_context(|| format!("can't write in {}", directory.display()))?;
    file.write_all(&scrubbed.bytes)?;
    file.as_file()
        .set_permissions(Permissions::from_mode(MODE))?;
    file.as_file().sync_all()?;
    file.persist_noclobber(&path)
        .map_err(|unmoved| unmoved.error)
        .context("can't move the upload into place")?;
    Ok(())
}

/// A new file in `directory` under a dotted name, which nginx doesn't serve.
fn unserved_file(directory: &Path) -> std::io::Result<NamedTempFile> {
    tempfile::Builder::new()
        .prefix(".scrubbing-")
        .tempfile_in(directory)
}

#[cfg(test)]
mod tests {
    use std::fs;

    use filehost::{ImageType, MAX_BYTES};

    use super::*;

    const ID: &str = "q7Hx2mZp9vLcT4wRfBa-_w";
    const PHOTO: &[u8] = include_bytes!("../fixtures/photo.jpg");

    /// What concierge sends for `bytes`, all of it or cut off.
    fn request(bytes: &[u8], ended: bool) -> Vec<u8> {
        let mut request = handoff::head(&UploadId::parse(ID).unwrap()).into_bytes();
        for chunk in bytes.chunks(1000) {
            request.extend(handoff::frame_header(chunk.len()));
            request.extend(chunk);
        }
        if ended {
            request.extend(handoff::END);
        }
        request
    }

    /// Takes `request` into a new directory, answering the answer, what it
    /// wrote back and the directory's files.
    fn take_into(request: &[u8]) -> (anyhow::Result<Answer>, String, Vec<String>) {
        let directory = tempfile::tempdir().unwrap();
        let mut output = Vec::new();
        let answer = take(directory.path(), request, &mut output);
        let mut files: Vec<_> = fs::read_dir(directory.path())
            .unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .collect();
        files.sort();
        (answer, String::from_utf8(output).unwrap(), files)
    }

    #[test]
    fn stores_a_scrubbed_photo_under_its_id_and_answers_its_size() {
        let directory = tempfile::tempdir().unwrap();
        let mut output = Vec::new();

        let answer = take(directory.path(), &request(PHOTO, true)[..], &mut output).unwrap();

        let scrubbed = scrub(PHOTO).unwrap();
        let size = scrubbed.bytes.len() as u64;
        assert_eq!(
            answer,
            Answer::Stored {
                kind: ImageType::Jpeg,
                size
            }
        );
        assert_eq!(output, answer.line().as_bytes());
        let path = directory.path().join(format!("{ID}.jpg"));
        assert_eq!(fs::read(&path).unwrap(), scrubbed.bytes);
        let mode = fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, MODE);
        assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 1);
    }

    #[test]
    fn stores_nothing_of_what_isnt_an_image() {
        let svg = br#"<svg xmlns="http://www.w3.org/2000/svg"/>"#;
        let (answer, output, files) = take_into(&request(svg, true));
        assert_eq!(answer.unwrap(), Answer::NotAnImage);
        assert_eq!(output, "not-an-image\n");
        assert!(files.is_empty(), "{files:?}");
    }

    #[test]
    fn stores_nothing_of_an_upload_past_the_limit() {
        let (answer, output, files) = take_into(&request(&vec![0; MAX_BYTES + 1], true));
        assert_eq!(answer.unwrap(), Answer::TooLarge);
        assert_eq!(output, "too-large\n");
        assert!(files.is_empty(), "{files:?}");
    }

    #[test]
    fn answers_nothing_and_stores_nothing_of_an_upload_cut_off() {
        let (answer, output, files) = take_into(&request(PHOTO, false));
        assert!(answer.is_err());
        assert_eq!(output, "");
        assert!(files.is_empty(), "{files:?}");
    }

    #[test]
    fn writes_under_a_dotted_name_until_the_upload_is_whole() {
        let directory = tempfile::tempdir().unwrap();
        let file = unserved_file(directory.path()).unwrap();
        let name = file.path().file_name().unwrap().to_str().unwrap();
        assert!(name.starts_with('.'), "{name}");
    }

    #[test]
    fn never_replaces_a_file_already_stored() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join(format!("{ID}.jpg"));
        fs::write(&path, "someone else's").unwrap();

        let answer = take(directory.path(), &request(PHOTO, true)[..], Vec::new());

        assert!(answer.is_err());
        assert_eq!(fs::read(&path).unwrap(), b"someone else's");
        assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 1);
    }
}
