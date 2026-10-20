//! PNG: a signature and chunks, whose CRCs img-parts checks.

use img_parts::{Bytes, ImageEXIF, png::Png};

use crate::exif;

/// The chunks that draw the image, animate it or give its colours:
/// everything else, text and EXIF and timestamps among them, goes.
const KEPT: [&[u8; 4]; 18] = [
    b"IHDR", b"PLTE", b"IDAT", b"IEND", b"tRNS", b"cHRM", b"gAMA", b"iCCP", b"sBIT", b"sRGB",
    b"cICP", b"mDCV", b"cLLI", b"bKGD", b"pHYs", b"acTL", b"fcTL", b"fdAT",
];

/// `upload` with only the chunks in [`KEPT`] and its orientation, if
/// it's a PNG with a header first, data, an end last and nothing after.
pub fn scrub(upload: &[u8]) -> Option<Vec<u8>> {
    let mut png = Png::from_bytes(Bytes::copy_from_slice(upload)).ok()?;
    let kinds: Vec<_> = png.chunks().iter().map(|chunk| chunk.kind()).collect();
    let well_formed = kinds.first() == Some(b"IHDR")
        && kinds.last() == Some(b"IEND")
        && kinds.contains(b"IDAT")
        && png.len() == upload.len();
    if !well_formed {
        return None;
    }
    let orientation = png.exif().and_then(|tiff| exif::orientation_only(&tiff));
    png.chunks_mut()
        .retain(|chunk| KEPT.contains(&&chunk.kind()));
    png.set_exif(orientation.map(Bytes::from));
    Some(png.encoder().bytes().to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::{assert_idempotent, assert_same_pixels, assert_stripped};

    const PHOTO: &[u8] = include_bytes!("../fixtures/photo.png");

    fn kinds(png: &[u8]) -> Vec<String> {
        let png = Png::from_bytes(Bytes::copy_from_slice(png)).unwrap();
        let kinds = png.chunks().iter().map(|chunk| chunk.kind());
        kinds
            .map(|kind| String::from_utf8_lossy(&kind).into_owned())
            .collect()
    }

    #[test]
    fn strips_text_xmp_and_exif_down_to_the_drawing() {
        let scrubbed = scrub(PHOTO).unwrap();

        assert_stripped(PHOTO, &scrubbed);
        assert!(
            kinds(PHOTO).iter().any(|kind| kind == "eXIf"),
            "{:?}",
            kinds(PHOTO)
        );
        assert!(
            kinds(&scrubbed)
                .iter()
                .all(|kind| KEPT.contains(&kind.as_bytes().try_into().unwrap()))
        );
        assert_same_pixels(PHOTO, &scrubbed);
        assert_idempotent(&scrubbed);
    }

    #[test]
    fn keeps_a_turned_screenshots_orientation_alone() {
        let mut png = Png::from_bytes(Bytes::from_static(PHOTO)).unwrap();
        png.set_exif(Some(crate::exif::tests::turned().into()));
        let turned = png.encoder().bytes();

        let scrubbed = scrub(&turned).unwrap();

        assert_stripped(&turned, &scrubbed);
        let kept = Png::from_bytes(scrubbed.into()).unwrap().exif().unwrap();
        assert_eq!(kept.len(), 26);
        assert_eq!(crate::exif::tests::orientation(&kept), Some(6));
    }

    #[test]
    fn refuses_a_png_with_something_after_its_end() {
        let polyglot = [PHOTO, b"<html><script>alert(1)</script></html>"].concat();
        assert_eq!(scrub(&polyglot), None);
    }

    #[test]
    fn refuses_a_png_cut_short_or_corrupted() {
        assert_eq!(scrub(&PHOTO[..PHOTO.len() - 12]), None);
        let mut corrupted = PHOTO.to_vec();
        corrupted[40] ^= 0xff;
        assert_eq!(scrub(&corrupted), None);
    }
}
