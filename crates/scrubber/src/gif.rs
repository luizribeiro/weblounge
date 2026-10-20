//! GIF: a header, a screen descriptor and colour table, then blocks, each
//! an extension or an image, their data in length-prefixed sub-blocks,
//! and a trailer. It's walked by hand, as img-parts doesn't read GIF; the
//! LZW data of images is copied without being decoded.

const EXTENSION: u8 = 0x21;
const IMAGE: u8 = 0x2c;
const TRAILER: u8 = 0x3b;
const GRAPHIC_CONTROL: u8 = 0xf9;
const APPLICATION: u8 = 0xff;
/// The application extensions that say how often an animation loops.
const LOOPING: [&[u8]; 2] = [b"NETSCAPE2.0", b"ANIMEXTS1.0"];

/// `upload` with only its images, their timing and how often they loop,
/// if it's a GIF with an image and nothing after its trailer: comments,
/// plain text, which browsers don't draw, XMP and other applications'
/// extensions go.
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
                if let Some(kept) = kept_extension(label, &upload[at + 2..end]) {
                    scrubbed.extend(kept);
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

/// The extension labelled `label`, whose sub-blocks are `sub_blocks`,
/// rebuilt from just the sub-blocks that time a frame or loop an
/// animation, if it's one that does: whatever else it carries goes with
/// every other extension.
fn kept_extension(label: u8, sub_blocks: &[u8]) -> Option<Vec<u8>> {
    let blocks = split_sub_blocks(sub_blocks);
    let kept = match (label, blocks.as_slice()) {
        (GRAPHIC_CONTROL, [control, ..]) if control.len() == 4 => vec![*control],
        (APPLICATION, [name, looping, ..])
            if LOOPING.contains(name) && looping.len() == 3 && looping[0] == 1 =>
        {
            vec![*name, *looping]
        }
        _ => return None,
    };
    let mut extension = vec![EXTENSION, label];
    for block in kept {
        extension.push(u8::try_from(block.len()).expect("a sub-block is shorter than 256 bytes"));
        extension.extend(block);
    }
    extension.push(0);
    Some(extension)
}

/// The data of each sub-block in `sub_blocks`, which [`sub_blocks_end`]
/// found well-formed.
fn split_sub_blocks(mut sub_blocks: &[u8]) -> Vec<&[u8]> {
    let mut blocks = Vec::new();
    while let Some((&length, rest)) = sub_blocks.split_first()
        && length > 0
    {
        let (block, after) = rest.split_at(usize::from(length));
        blocks.push(block);
        sub_blocks = after;
    }
    blocks
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

    /// `ANIMATED` with `extra`, a sub-block of `payload`, added to the
    /// extension that starts with `start`.
    fn smuggling(start: &[u8], payload: &[u8]) -> Vec<u8> {
        let at = ANIMATED
            .windows(start.len())
            .position(|window| window == start)
            .unwrap();
        let end = sub_blocks_end(ANIMATED, at + 2).unwrap();
        let extra = [&[payload.len() as u8][..], payload].concat();
        [&ANIMATED[..end - 1], &extra, &ANIMATED[end - 1..]].concat()
    }

    #[test]
    fn keeps_nothing_smuggled_into_the_extensions_it_keeps() {
        let looping = b"\x21\xff\x0bNETSCAPE2.0";
        let timing = b"\x21\xf9\x04";
        for start in [&looping[..], timing] {
            let smuggled = smuggling(start, b"the-xmp-canary");
            let scrubbed = scrub(&smuggled).unwrap();
            assert_stripped(&smuggled, &scrubbed);
            assert_eq!(scrubbed, scrub(ANIMATED).unwrap());
        }
    }

    #[test]
    fn drops_plain_text_which_browsers_dont_draw() {
        let text = [&b"\x21\x01\x0c"[..], &[0; 12], b"\x0ethe-xmp-canary\x00"].concat();
        let at = ANIMATED.len() - 1;
        let with_text = [&ANIMATED[..at], &text, &ANIMATED[at..]].concat();
        let scrubbed = scrub(&with_text).unwrap();
        assert_stripped(&with_text, &scrubbed);
        assert_eq!(scrubbed, scrub(ANIMATED).unwrap());
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
