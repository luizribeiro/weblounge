//! A scrubber for concierge's tests, on a socket in a temporary
//! directory. It takes whatever starts like an image and stores it as it
//! is, unscrubbed.

use std::{
    fs,
    io::Write,
    os::unix::net::{UnixListener, UnixStream},
    path::{Path, PathBuf},
    thread,
};

use tempfile::TempDir;

use crate::{
    ImageType,
    handoff::{Answer, ReadError, read_upload},
};

pub struct FakeScrubber {
    /// Where concierge hands uploads over.
    pub socket: PathBuf,
    /// Where it stores them.
    pub uploads: PathBuf,
    _dir: TempDir,
}

impl FakeScrubber {
    pub fn start() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let socket = dir.path().join("scrubber.sock");
        let uploads = dir.path().join("uploads");
        fs::create_dir(&uploads).unwrap();
        let listener = UnixListener::bind(&socket).unwrap();
        let directory = uploads.clone();
        thread::spawn(move || {
            for connection in listener.incoming().map_while(Result::ok) {
                let directory = directory.clone();
                thread::spawn(move || take(&directory, connection));
            }
        });
        Self {
            socket,
            uploads,
            _dir: dir,
        }
    }

    /// The names of the files it stored.
    pub fn stored(&self) -> Vec<String> {
        let mut names: Vec<_> = fs::read_dir(&self.uploads)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .collect();
        names.sort();
        names
    }
}

fn take(directory: &Path, mut connection: UnixStream) {
    let answer = match read_upload(&connection) {
        Ok(upload) => match ImageType::sniff(&upload.bytes) {
            Some(kind) => {
                let path = directory.join(upload.id.file_name(kind));
                fs::write(path, &upload.bytes).unwrap();
                Answer::Stored {
                    kind,
                    size: upload.bytes.len() as u64,
                }
            }
            None => Answer::NotAnImage,
        },
        Err(ReadError::TooLarge) => Answer::TooLarge,
        Err(_) => return,
    };
    connection.write_all(answer.line().as_bytes()).ok();
}
