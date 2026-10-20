//! JPEG: markers and the segments they start, and after each scan header
//! the entropy-coded data up to the next marker. It's walked by hand:
//! img-parts keeps all that follows the first scan as it is, and phones
//! put more after a photo's end, a second picture or a video, with
//! metadata of its own. That goes, so a photo with something after its
//! end isn't refused, as other images with something after theirs are.

use crate::exif;

const MARKER: u8 = 0xff;
const SOI: u8 = 0xd8;
const EOI: u8 = 0xd9;
const SOS: u8 = 0xda;
const EXIF: &[u8] = b"Exif\0\0";

/// What becomes of a segment.
#[derive(Debug, PartialEq, Eq)]
enum Fate {
    Kept,
    /// EXIF, of which only the orientation is kept.
    Exif,
    Dropped,
    /// No JPEG has it.
    Refused,
}

/// What becomes of a segment with `marker` and `payload`: those that draw
/// the image or give its colours stay; APP segments are kept only for
/// JFIF, an ICC profile or Adobe's colour transform, so XMP, IPTC,
/// thumbnails and the index of pictures after the end go, as do
/// comments.
fn fate(marker: u8, payload: &[u8]) -> Fate {
    let kept_if = |kept: bool| if kept { Fate::Kept } else { Fate::Dropped };
    match marker {
        0xe0 => kept_if(payload.starts_with(b"JFIF\0")),
        0xe1 if payload.starts_with(EXIF) => Fate::Exif,
        0xe2 => kept_if(payload.starts_with(b"ICC_PROFILE\0")),
        0xee => kept_if(payload.starts_with(b"Adobe")),
        0xe1..=0xef | 0xfe | 0xc8 | 0xf0..=0xfd => Fate::Dropped,
        0xc0..=0xcf | 0xda..=0xdf => Fate::Kept,
        _ => Fate::Refused,
    }
}

/// Start of frame markers, which DHT, JPG and DAC share the range of.
fn starts_a_frame(marker: u8) -> bool {
    matches!(marker, 0xc0..=0xcf) && !matches!(marker, 0xc4 | 0xc8 | 0xcc)
}

/// `upload` up to its end, with only the segments [`fate`] keeps and its
/// orientation, if it's a JPEG with a frame and a scan.
pub fn scrub(upload: &[u8]) -> Option<Vec<u8>> {
    let mut at = upload.starts_with(&[MARKER, SOI]).then_some(2)?;
    let mut scrubbed = vec![MARKER, SOI];
    let (mut framed, mut scanned, mut exif_seen) = (false, false, false);
    loop {
        if *upload.get(at)? != MARKER {
            return None;
        }
        while *upload.get(at)? == MARKER {
            at += 1;
        }
        let marker = upload[at];
        at += 1;
        match marker {
            EOI => {
                scrubbed.extend([MARKER, EOI]);
                return (framed && scanned).then_some(scrubbed);
            }
            0xd0..=0xd7 | 0x01 => {
                scrubbed.extend([MARKER, marker]);
                continue;
            }
            _ => {}
        }
        let length = usize::from(u16::from_be_bytes(upload.get(at..at + 2)?.try_into().ok()?));
        let payload = upload.get(at + 2..at.checked_add(length)?)?;
        match fate(marker, payload) {
            Fate::Kept => {
                scrubbed.extend([MARKER, marker]);
                scrubbed.extend(&upload[at..at + length]);
            }
            Fate::Exif if !exif_seen => {
                exif_seen = true;
                let tiff = exif::orientation_only(&payload[EXIF.len()..]);
                if let Some(tiff) = tiff {
                    let length = 2 + EXIF.len() + tiff.len();
                    scrubbed.extend([MARKER, 0xe1]);
                    scrubbed.extend((length as u16).to_be_bytes());
                    scrubbed.extend(EXIF);
                    scrubbed.extend(tiff);
                }
            }
            Fate::Exif | Fate::Dropped => {}
            Fate::Refused => return None,
        }
        at += length;
        framed |= starts_a_frame(marker);
        if marker == SOS {
            scanned = true;
            let end = entropy_end(upload, at)?;
            scrubbed.extend(&upload[at..end]);
            at = end;
        }
    }
}

/// Where the entropy-coded data starting at `from` ends: at the first
/// marker, as within it a 0xff is followed by a stuffed zero or a restart.
fn entropy_end(upload: &[u8], from: usize) -> Option<usize> {
    let mut at = from;
    loop {
        let marker = at + upload[at..].iter().position(|&byte| byte == MARKER)?;
        match *upload.get(marker + 1)? {
            0x00 | 0xd0..=0xd7 => at = marker + 2,
            _ => return Some(marker),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::{assert_idempotent, assert_same_pixels, assert_stripped, contains};

    const PHOTO: &[u8] = include_bytes!("../fixtures/photo.jpg");
    const PROGRESSIVE: &[u8] = include_bytes!("../fixtures/progressive.jpg");

    /// The TIFF of the first EXIF segment in `jpeg`, if any.
    fn exif_of(jpeg: &[u8]) -> Option<&[u8]> {
        let at = jpeg
            .windows(2 + 2 + EXIF.len())
            .position(|window| window.starts_with(&[MARKER, 0xe1]) && window.ends_with(EXIF))?;
        let length = usize::from(u16::from_be_bytes([jpeg[at + 2], jpeg[at + 3]]));
        Some(&jpeg[at + 4 + EXIF.len()..at + 2 + length])
    }

    fn scans(jpeg: &[u8]) -> usize {
        jpeg.windows(2)
            .filter(|window| window == &[MARKER, SOS])
            .count()
    }

    #[test]
    fn strips_a_phone_photo_of_gps_exif_xmp_iptc_and_comments() {
        let scrubbed = scrub(PHOTO).unwrap();

        assert_stripped(PHOTO, &scrubbed);
        let gps_directory = [0x88, 0x25];
        assert!(contains(exif_of(PHOTO).unwrap(), &gps_directory));
        assert!(!contains(exif_of(&scrubbed).unwrap(), &gps_directory));
        assert_same_pixels(PHOTO, &scrubbed);
        assert_idempotent(&scrubbed);
    }

    #[test]
    fn keeps_the_orientation_of_a_photo_taken_on_its_side_alone() {
        let kept = exif_of(&scrub(PHOTO).unwrap()).unwrap().to_vec();
        assert_eq!(crate::exif::tests::orientation(&kept), Some(6));
    }

    #[test]
    fn keeps_every_scan_of_a_progressive_photo() {
        let scrubbed = scrub(PROGRESSIVE).unwrap();

        assert!(scans(PROGRESSIVE) > 1);
        assert_eq!(scans(&scrubbed), scans(PROGRESSIVE));
        assert_stripped(PROGRESSIVE, &scrubbed);
        assert_same_pixels(PROGRESSIVE, &scrubbed);
        assert_eq!(exif_of(&scrubbed), None);
    }

    #[test]
    fn drops_what_a_phone_put_after_the_photos_end() {
        let with_second_picture = [PHOTO, PHOTO].concat();
        assert_eq!(scrub(&with_second_picture), scrub(PHOTO));
        let with_html = [PHOTO, b"<script>alert(1)</script>"].concat();
        assert_eq!(scrub(&with_html), scrub(PHOTO));
    }

    #[test]
    fn refuses_a_photo_cut_short() {
        for cut in [3, 20, PHOTO.len() / 2, PHOTO.len() - 1] {
            assert_eq!(scrub(&PHOTO[..cut]), None, "{cut}");
        }
    }

    #[test]
    fn refuses_a_jpeg_with_no_picture_in_it() {
        let only_metadata = [&PHOTO[..2], b"\xff\xfe\x00\x04hi", b"\xff\xd9"].concat();
        assert_eq!(scrub(&only_metadata), None);
    }
}
