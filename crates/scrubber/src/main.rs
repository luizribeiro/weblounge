use std::{io, path::PathBuf, process::ExitCode};

use filehost::handoff::Answer;

/// Takes one upload on standard input, the connection systemd accepted on
/// the scrubber's socket, stores it in CONCIERGE_UPLOADS if it's an image,
/// and answers on standard output. It logs what it stored, but never an
/// upload's id, which is all that keeps it private.
fn main() -> ExitCode {
    let Some(directory) = std::env::var_os("CONCIERGE_UPLOADS").map(PathBuf::from) else {
        eprintln!("CONCIERGE_UPLOADS is not set");
        return ExitCode::FAILURE;
    };
    match scrubber::take(&directory, io::stdin().lock(), io::stdout().lock()) {
        Ok(Answer::Stored { kind, size }) => {
            eprintln!("Stored an upload as {}, {size} bytes", kind.extension());
            ExitCode::SUCCESS
        }
        Ok(answer) => {
            eprintln!("Refused an upload: {}", answer.line().trim_end());
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("Storing an upload failed: {error:#}");
            ExitCode::FAILURE
        }
    }
}
