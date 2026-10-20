//! The scrubber as systemd runs it: one upload on standard input, the
//! answer on standard output.

use std::{
    io::Write,
    process::{Command, Output, Stdio},
};

use filehost::{UploadId, handoff};

const PHOTO: &[u8] = include_bytes!("../fixtures/animated.gif");

fn run(directory: Option<&std::path::Path>, request: &[u8]) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_scrubber"));
    command.env_clear();
    if let Some(directory) = directory {
        command.env("CONCIERGE_UPLOADS", directory);
    }
    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    if let Err(error) = child.stdin.take().unwrap().write_all(request) {
        assert_eq!(error.kind(), std::io::ErrorKind::BrokenPipe);
    }
    child.wait_with_output().unwrap()
}

fn request(id: &UploadId, bytes: &[u8]) -> Vec<u8> {
    let mut request = handoff::head(id).into_bytes();
    request.extend(handoff::frame_header(bytes.len()));
    request.extend(bytes);
    request.extend(handoff::END);
    request
}

#[test]
fn stores_an_upload_and_answers_without_logging_its_id() {
    let directory = tempfile::tempdir().unwrap();
    let id = UploadId::generate();

    let output = run(Some(directory.path()), &request(&id, PHOTO));

    assert!(output.status.success(), "{output:?}");
    let stored = std::fs::read(directory.path().join(format!("{}.gif", id.as_str()))).unwrap();
    let answer = String::from_utf8(output.stdout).unwrap();
    assert_eq!(answer, format!("stored gif {}\n", stored.len()));
    let log = String::from_utf8(output.stderr).unwrap();
    assert_eq!(
        log,
        format!("Stored an upload as gif, {} bytes\n", stored.len())
    );
}

#[test]
fn fails_without_its_directory_or_a_whole_upload() {
    let id = UploadId::generate();
    let output = run(None, &request(&id, PHOTO));
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());

    let directory = tempfile::tempdir().unwrap();
    let cut_off = request(&id, PHOTO);
    let output = run(Some(directory.path()), &cut_off[..cut_off.len() - 4]);
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    let log = String::from_utf8(output.stderr).unwrap();
    assert!(!log.contains(id.as_str()), "{log}");
    assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 0);
}
