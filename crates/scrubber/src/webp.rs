//! WebP: a RIFF container of chunks, which img-parts reads.

use img_parts::{
    Bytes, ImageEXIF,
    riff::{RiffChunk, RiffContent},
    webp::{
        CHUNK_ALPH, CHUNK_ANIM, CHUNK_ANMF, CHUNK_ICCP, CHUNK_VP8, CHUNK_VP8L, CHUNK_VP8X, WebP,
    },
};

use crate::exif;

/// The chunks that draw the image, animate it or give its colours, so
/// EXIF, XMP and whatever else an encoder adds go.
const KEPT: [[u8; 4]; 7] = [
    CHUNK_VP8X, CHUNK_VP8, CHUNK_VP8L, CHUNK_ALPH, CHUNK_ANIM, CHUNK_ANMF, CHUNK_ICCP,
];

/// The bit of the VP8X chunk's flags that says XMP follows, which
/// img-parts leaves as it was.
const XMP_FLAG: u8 = 0b0000_0100;

/// `upload` with only the chunks in [`KEPT`] and its orientation, if it's
/// a WebP that draws something and has nothing after its end.
pub fn scrub(upload: &[u8]) -> Option<Vec<u8>> {
    let mut webp = WebP::from_bytes(Bytes::copy_from_slice(upload)).ok()?;
    let ids: Vec<_> = webp.chunks().iter().map(RiffChunk::id).collect();
    let draws = match *ids.first()? {
        CHUNK_VP8 | CHUNK_VP8L => true,
        CHUNK_VP8X => ids
            .iter()
            .any(|id| matches!(*id, CHUNK_VP8 | CHUNK_VP8L | CHUNK_ANMF)),
        _ => false,
    };
    if !draws || webp.len() as usize != upload.len() {
        return None;
    }
    let orientation = webp.exif().and_then(|tiff| exif::orientation_only(&tiff));
    webp.chunks_mut().retain(|chunk| KEPT.contains(&chunk.id()));
    webp.set_exif(orientation.map(Bytes::from));
    if let Some(RiffContent::Data(flags)) = webp
        .chunks_mut()
        .iter_mut()
        .find(|chunk| chunk.id() == CHUNK_VP8X)
        .map(RiffChunk::content_mut)
    {
        let mut cleared = flags.to_vec();
        *cleared.first_mut()? &= !XMP_FLAG;
        *flags = cleared.into();
    }
    Some(webp.encoder().bytes().to_vec())
}

#[cfg(test)]
mod tests {
    use filehost::ImageType;

    use super::*;
    use crate::tests::{
        assert_idempotent, assert_same_frames, assert_same_pixels, assert_stripped,
    };

    const ANIMATED: &[u8] = include_bytes!("../fixtures/animated.webp");
    const STILL: &[u8] = include_bytes!("../fixtures/still.webp");
    const EXIF_FLAG: u8 = 0b0000_1000;

    fn parse(webp: &[u8]) -> WebP {
        WebP::from_bytes(Bytes::copy_from_slice(webp)).unwrap()
    }

    fn count(webp: &WebP, id: [u8; 4]) -> usize {
        webp.chunks_by_id(id).count()
    }

    fn flags(webp: &WebP) -> u8 {
        webp.chunk_by_id(CHUNK_VP8X)
            .unwrap()
            .content()
            .data()
            .unwrap()[0]
    }

    #[test]
    fn an_animation_stays_animated_without_its_exif_and_xmp() {
        let scrubbed = scrub(ANIMATED).unwrap();

        assert_stripped(ANIMATED, &scrubbed);
        let (before, after) = (parse(ANIMATED), parse(&scrubbed));
        assert_eq!(count(&before, CHUNK_ANMF), 3);
        assert_eq!(count(&after, CHUNK_ANMF), 3);
        assert_eq!(count(&after, CHUNK_ANIM), 1);
        assert_ne!(flags(&before) & (XMP_FLAG | EXIF_FLAG), 0);
        assert_eq!(flags(&after) & (XMP_FLAG | EXIF_FLAG), 0);
        assert_same_frames(ImageType::Webp, ANIMATED, &scrubbed);
        assert_idempotent(&scrubbed);
    }

    #[test]
    fn a_still_keeps_its_orientation_alone() {
        let scrubbed = scrub(STILL).unwrap();

        assert_stripped(STILL, &scrubbed);
        let after = parse(&scrubbed);
        let kept = after.exif().unwrap();
        assert_eq!(crate::exif::tests::orientation(&kept), Some(6));
        assert_eq!(flags(&after) & (XMP_FLAG | EXIF_FLAG), EXIF_FLAG);
        assert_same_pixels(STILL, &scrubbed);
        assert_idempotent(&scrubbed);
    }

    #[test]
    fn refuses_a_webp_with_something_after_its_end() {
        let polyglot = [ANIMATED, b"<script>alert(1)</script>!"].concat();
        assert_eq!(scrub(&polyglot), None);
    }

    #[test]
    fn refuses_a_webp_cut_short_or_with_nothing_to_draw() {
        assert_eq!(scrub(&STILL[..STILL.len() - 10]), None);
        let mut only_metadata = parse(STILL);
        only_metadata
            .chunks_mut()
            .retain(|chunk| chunk.id() != CHUNK_VP8);
        assert_eq!(scrub(&only_metadata.encoder().bytes()), None);
    }
}
