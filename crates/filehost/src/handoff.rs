//! How concierge hands an upload to the scrubber, over a connection to the
//! scrubber's socket: the upload's id on a line of its own, then its bytes
//! in frames, each a four-byte big-endian length and that many bytes, and
//! last an empty frame. A connection that ends before the empty frame was
//! cut off, and nothing of it is stored. The scrubber answers one line.

use std::io::{self, BufRead, BufReader, Read};

use crate::{ImageType, MAX_BYTES, UploadId};

/// The frame that ends an upload.
pub const END: [u8; 4] = [0; 4];

/// What concierge sends before the upload's bytes.
pub fn head(id: &UploadId) -> String {
    format!("{}\n", id.as_str())
}

/// What concierge sends before `length` of the upload's bytes.
pub fn frame_header(length: usize) -> [u8; 4] {
    u32::try_from(length)
        .expect("a frame is shorter than 4 GiB")
        .to_be_bytes()
}

/// An upload as the scrubber reads it.
#[derive(Debug)]
pub struct Upload {
    pub id: UploadId,
    pub bytes: Vec<u8>,
}

#[derive(Debug)]
pub enum ReadError {
    TooLarge,
    /// The connection ended before the last frame.
    CutOff,
    /// The first line isn't an id.
    Malformed,
    Io(io::Error),
}

impl From<io::Error> for ReadError {
    fn from(error: io::Error) -> Self {
        match error.kind() {
            io::ErrorKind::UnexpectedEof => Self::CutOff,
            _ => Self::Io(error),
        }
    }
}

/// Reads an upload of up to [`MAX_BYTES`] from `reader`.
pub fn read_upload(reader: impl Read) -> Result<Upload, ReadError> {
    let mut reader = BufReader::new(reader);
    let mut line = Vec::new();
    let line_length = UploadId::LENGTH as u64 + 1;
    (&mut reader)
        .take(line_length)
        .read_until(b'\n', &mut line)?;
    let id = line
        .strip_suffix(b"\n")
        .and_then(|id| std::str::from_utf8(id).ok())
        .and_then(UploadId::parse)
        .ok_or(ReadError::Malformed)?;
    let mut bytes = Vec::new();
    loop {
        let mut header = [0; 4];
        reader.read_exact(&mut header)?;
        let length = u32::from_be_bytes(header) as usize;
        if length == 0 {
            return Ok(Upload { id, bytes });
        }
        if bytes.len() + length > MAX_BYTES {
            return Err(ReadError::TooLarge);
        }
        let start = bytes.len();
        bytes.resize(start + length, 0);
        reader.read_exact(&mut bytes[start..])?;
    }
}

/// The scrubber's answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Answer {
    /// Stored as `kind`, `size` bytes once its metadata was stripped.
    Stored {
        kind: ImageType,
        size: u64,
    },
    /// Not one of the images the network takes, or not a well-formed one.
    NotAnImage,
    TooLarge,
}

impl Answer {
    pub fn line(&self) -> String {
        match self {
            Self::Stored { kind, size } => format!("stored {} {size}\n", kind.extension()),
            Self::NotAnImage => "not-an-image\n".into(),
            Self::TooLarge => "too-large\n".into(),
        }
    }

    pub fn parse(line: &str) -> Option<Self> {
        let words: Vec<_> = line.strip_suffix('\n')?.split(' ').collect();
        match words[..] {
            ["stored", extension, size] => Some(Self::Stored {
                kind: ImageType::from_extension(extension)?,
                size: size.parse().ok()?,
            }),
            ["not-an-image"] => Some(Self::NotAnImage),
            ["too-large"] => Some(Self::TooLarge),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ID: &str = "q7Hx2mZp9vLcT4wRfBa-_w";

    /// What concierge sends for an upload of `chunks`, ended or cut off.
    fn sent(chunks: &[&[u8]], ended: bool) -> Vec<u8> {
        let mut sent = head(&UploadId::parse(ID).unwrap()).into_bytes();
        for chunk in chunks {
            sent.extend(frame_header(chunk.len()));
            sent.extend(*chunk);
        }
        if ended {
            sent.extend(END);
        }
        sent
    }

    #[test]
    fn reads_the_id_and_the_bytes_of_every_frame() {
        let upload = read_upload(&sent(&[b"\x89PNG", b"\r\n\x1a\n"], true)[..]).unwrap();
        assert_eq!(upload.id.as_str(), ID);
        assert_eq!(upload.bytes, b"\x89PNG\r\n\x1a\n");
        let empty = read_upload(&sent(&[], true)[..]).unwrap();
        assert!(empty.bytes.is_empty());
    }

    #[test]
    fn an_upload_without_its_last_frame_was_cut_off() {
        let cut_off = sent(&[b"\x89PNG\r\n\x1a\n"], false);
        assert!(matches!(read_upload(&cut_off[..]), Err(ReadError::CutOff)));
        let mid_frame = &cut_off[..cut_off.len() - 2];
        assert!(matches!(read_upload(mid_frame), Err(ReadError::CutOff)));
    }

    #[test]
    fn stops_reading_past_the_limit() {
        let most = vec![0; MAX_BYTES];
        assert_eq!(
            read_upload(&sent(&[&most], true)[..]).unwrap().bytes.len(),
            MAX_BYTES
        );
        let over = sent(&[&most[..MAX_BYTES / 2], &most[..MAX_BYTES / 2 + 1]], true);
        assert!(matches!(read_upload(&over[..]), Err(ReadError::TooLarge)));
    }

    #[test]
    fn refuses_a_first_line_that_isnt_an_id() {
        for head in [
            &b"../../etc/passwd\n"[..],
            b"q7Hx2mZp9vLcT4wRfBa-_wq7Hx2mZp9vLcT4wRfBa-_w\n",
            b"\n",
        ] {
            let mut sent = head.to_vec();
            sent.extend(END);
            assert!(
                matches!(read_upload(&sent[..]), Err(ReadError::Malformed)),
                "{head:?}"
            );
        }
    }

    #[test]
    fn answers_round_trip_through_their_lines() {
        let answers = [
            Answer::Stored {
                kind: ImageType::Webp,
                size: 48_213,
            },
            Answer::NotAnImage,
            Answer::TooLarge,
        ];
        for answer in answers {
            assert_eq!(Answer::parse(&answer.line()), Some(answer), "{answer:?}");
        }
        assert_eq!(answers[0].line(), "stored webp 48213\n");
        for line in [
            "",
            "stored\n",
            "stored svg 12\n",
            "stored png -1\n",
            "too-large",
        ] {
            assert_eq!(Answer::parse(line), None, "{line:?}");
        }
    }
}
