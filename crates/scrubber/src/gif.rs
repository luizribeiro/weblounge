//! GIF: a header, a screen descriptor and colour table, then blocks, each
//! an extension or an image, their data in length-prefixed sub-blocks,
//! and a trailer. It's walked by hand, as img-parts doesn't read GIF; the
//! LZW data of images is copied without being decoded.

const EXTENSION: u8 = 0x21;
const IMAGE: u8 = 0x2c;
const TRAILER: u8 = 0x3b;
const GRAPHIC_CONTROL: u8 = 0xf9;
const PLAIN_TEXT: u8 = 0x01;
const APPLICATION: u8 = 0xff;
/// The application extensions that say how often an animation loops.
const LOOPING: [&[u8]; 2] = [b"NETSCAPE2.0", b"ANIMEXTS1.0"];

/// `upload` with only its images, their timing and how often they loop,
/// if it's a GIF with an image and nothing after its trailer: comments,
/// XMP and other applications' extensions go.
pub fn scrub(upload: &[u8]) -> Option<Vec<u8>> {
    let header = 6 + 7;
    let screen_flags = *upload.get(10)?;
    let mut at = header + colour_table_length(screen_flags);
    let mut scrubbed = upload.get(..at)?.to_vec();
    let mut images = 0;
    loop {
        let block = *upload.get(at)?;
        match block {
            EXTENSION => {
                let label = *upload.get(at + 1)?;
                let end = sub_blocks_end(upload, at + 2)?;
                let first_block = &upload[at + 2..end];
                let kept = match label {
                    GRAPHIC_CONTROL | PLAIN_TEXT => true,
                    APPLICATION => LOOPING.iter().any(|name| {
                        first_block.first() == Some(&11) && first_block[1..].starts_with(name)
                    }),
                    _ => false,
                };
                if kept {
                    scrubbed.extend(&upload[at..end]);
                }
                at = end;
            }
            IMAGE => {
                let descriptor = 10;
                let flags = *upload.get(at + 9)?;
                let data = at + descriptor + colour_table_length(flags) + 1;
                let end = sub_blocks_end(upload, data)?;
                scrubbed.extend(&upload[at..end]);
                images += 1;
                at = end;
            }
            TRAILER => {
                scrubbed.push(TRAILER);
                let ends_here = at + 1 == upload.len();
                return (ends_here && images > 0).then_some(scrubbed);
            }
            _ => return None,
        }
    }
}

/// The length of the colour table a descriptor's `flags` announce.
fn colour_table_length(flags: u8) -> usize {
    if flags & 0x80 == 0 {
        0
    } else {
        3 << ((flags & 0x07) + 1)
    }
}

/// Where the sub-blocks starting at `from` end, after the empty one.
fn sub_blocks_end(upload: &[u8], from: usize) -> Option<usize> {
    let mut at = from;
    loop {
        let length = usize::from(*upload.get(at)?);
        at += 1 + length;
        if length == 0 {
            return (at <= upload.len()).then_some(at);
        }
    }
}

#[cfg(test)]
mod tests {
    use filehost::ImageType;

    use super::*;
    use crate::tests::{assert_idempotent, assert_same_frames, assert_stripped, contains};

    const ANIMATED: &[u8] = include_bytes!("../fixtures/animated.gif");

    #[test]
    fn an_animation_stays_animated_and_looping_without_its_comment_and_xmp() {
        let scrubbed = scrub(ANIMATED).unwrap();

        assert_stripped(ANIMATED, &scrubbed);
        assert!(contains(ANIMATED, b"XMP DataXMP"));
        assert!(!contains(&scrubbed, b"XMP DataXMP"));
        assert!(contains(&scrubbed, b"NETSCAPE2.0"));
        assert_same_frames(ImageType::Gif, ANIMATED, &scrubbed);
        assert_idempotent(&scrubbed);
    }

    #[test]
    fn refuses_a_gif_with_something_after_its_trailer() {
        let polyglot = [ANIMATED, b"<script>alert(1)</script>"].concat();
        assert_eq!(scrub(&polyglot), None);
    }

    #[test]
    fn refuses_javascript_that_starts_like_a_gif() {
        assert_eq!(scrub(b"GIF89a=alert(document.domain)//;"), None);
        assert_eq!(
            scrub(b"GIF89a/*\x01\x00\x01\x00\x00\x00\x00*/=alert(1);"),
            None
        );
    }

    #[test]
    fn refuses_a_gif_cut_short_or_without_an_image() {
        for cut in [6, 13, ANIMATED.len() / 2, ANIMATED.len() - 1] {
            assert_eq!(scrub(&ANIMATED[..cut]), None, "{cut}");
        }
        let screen_only = [&ANIMATED[..13], &[TRAILER][..]].concat();
        assert_eq!(scrub(&screen_only), None);
    }
}
