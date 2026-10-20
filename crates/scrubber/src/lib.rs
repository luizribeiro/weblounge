//! The scrubber: the one process that reads what members upload. It tells
//! the images the network takes from anything else by their bytes, and
//! strips what they say about the photo and its taker (EXIF, XMP, GPS,
//! comments) without decoding a pixel. Each format keeps an allowlist of
//! what draws the image, so whatever an encoder adds that isn't on it
//! goes too.

mod exif;
mod gif;
mod jpeg;
mod png;
mod store;
mod webp;

use filehost::ImageType;

pub use store::take;

/// An upload stripped down to what draws it.
#[derive(Debug, PartialEq, Eq)]
pub struct Scrubbed {
    pub kind: ImageType,
    pub bytes: Vec<u8>,
}

/// `upload` without its metadata, if it's a well-formed image of a type
/// the network takes; anything else, a polyglot that is something more
/// besides, or an image cut short, is refused.
pub fn scrub(upload: &[u8]) -> Option<Scrubbed> {
    let kind = ImageType::sniff(upload)?;
    let bytes = match kind {
        ImageType::Png => png::scrub(upload),
        ImageType::Jpeg => jpeg::scrub(upload),
        ImageType::Gif => gif::scrub(upload),
        ImageType::Webp => webp::scrub(upload),
    }?;
    Some(Scrubbed { kind, bytes })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What a phone or an editor leaves in an image, which the fixtures
    /// all carry some of.
    pub const METADATA: [&[u8]; 6] = [
        b"Pixelphone",
        b"Zoe Example",
        b"2026:10:01 12:34:56",
        b"the-comment-canary",
        b"the-xmp-canary",
        b"the-iptc-canary",
    ];

    pub fn contains(haystack: &[u8], needle: &[u8]) -> bool {
        haystack
            .windows(needle.len())
            .any(|window| window == needle)
    }

    /// Asserts `scrubbed` holds none of the fixtures' metadata, and that
    /// `original` held some.
    pub fn assert_stripped(original: &[u8], scrubbed: &[u8]) {
        assert!(METADATA.iter().any(|text| contains(original, text)));
        for text in METADATA {
            assert!(
                !contains(scrubbed, text),
                "{:?} survived",
                String::from_utf8_lossy(text)
            );
        }
    }

    /// Asserts both images decode to the same pixels.
    pub fn assert_same_pixels(original: &[u8], scrubbed: &[u8]) {
        let decode = |bytes| image::load_from_memory(bytes).unwrap().to_rgba8();
        assert_eq!(decode(original), decode(scrubbed));
    }

    /// Asserts both animations of `kind` decode to the same frames.
    pub fn assert_same_frames(kind: ImageType, original: &[u8], scrubbed: &[u8]) {
        use image::{
            AnimationDecoder, RgbaImage,
            codecs::{gif::GifDecoder, webp::WebPDecoder},
        };
        let frames = |bytes: &[u8]| -> Vec<RgbaImage> {
            let bytes = std::io::Cursor::new(bytes.to_vec());
            let frames = match kind {
                ImageType::Gif => GifDecoder::new(bytes).unwrap().into_frames(),
                ImageType::Webp => WebPDecoder::new(bytes).unwrap().into_frames(),
                ImageType::Png | ImageType::Jpeg => unreachable!("{kind:?} isn't animated"),
            };
            frames.map(|frame| frame.unwrap().into_buffer()).collect()
        };
        let (before, after) = (frames(original), frames(scrubbed));
        assert!(before.len() > 1, "{} frames", before.len());
        assert_eq!(before, after);
    }

    /// Asserts scrubbing `upload` gives it back unchanged.
    pub fn assert_idempotent(upload: &[u8]) {
        let again = scrub(upload).unwrap();
        assert_eq!(again.bytes, upload);
    }

    #[test]
    fn refuses_what_isnt_an_image_whatever_it_is_called() {
        let svg = br#"<svg xmlns="http://www.w3.org/2000/svg" onload="alert(1)"><rect width="8" height="8"/></svg>"#;
        let html = b"<!doctype html><title>x</title><script>alert(document.domain)</script>";
        let pdf = b"%PDF-1.7\n1 0 obj << /Type /Catalog /Pages 2 0 R >> endobj\ntrailer << /Root 1 0 R >>\n%%EOF\n";
        for upload in [&svg[..], html, pdf, b"", b"\x89PNG"] {
            assert_eq!(scrub(upload), None, "{}", String::from_utf8_lossy(upload));
        }
    }

    #[test]
    fn refuses_html_dressed_up_as_each_image_type() {
        let html = b"<html><script>alert(document.domain)</script></html>";
        for magic in [
            &b"\x89PNG\r\n\x1a\n"[..],
            b"\xff\xd8\xff\xe0",
            b"GIF89a",
            b"RIFF\x40\x00\x00\x00WEBP",
        ] {
            let disguised = [magic, html].concat();
            assert_eq!(scrub(&disguised), None, "{magic:?}");
        }
    }
}
