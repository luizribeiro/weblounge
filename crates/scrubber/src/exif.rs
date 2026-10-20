//! EXIF, which phones fill with where and when a photo was taken. Of it,
//! uploads keep only the orientation, without which browsers show phone
//! photos on their side.

const ORIENTATION: u16 = 0x0112;
const SHORT: u16 = 3;
/// A TIFF header and a directory of one entry, with no next directory.
const ORIENTATION_ONLY_LENGTH: usize = 8 + 2 + 12 + 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ByteOrder {
    Big,
    Little,
}

impl ByteOrder {
    fn of(tiff: &[u8]) -> Option<Self> {
        [Self::Big, Self::Little]
            .into_iter()
            .find(|order| tiff.starts_with(order.header()))
    }

    fn header(self) -> &'static [u8; 4] {
        match self {
            Self::Big => b"MM\0*",
            Self::Little => b"II*\0",
        }
    }

    fn u16(self, bytes: [u8; 2]) -> u16 {
        match self {
            Self::Big => u16::from_be_bytes(bytes),
            Self::Little => u16::from_le_bytes(bytes),
        }
    }

    fn u32(self, bytes: [u8; 4]) -> u32 {
        match self {
            Self::Big => u32::from_be_bytes(bytes),
            Self::Little => u32::from_le_bytes(bytes),
        }
    }

    fn u16_bytes(self, value: u16) -> [u8; 2] {
        match self {
            Self::Big => value.to_be_bytes(),
            Self::Little => value.to_le_bytes(),
        }
    }

    fn u32_bytes(self, value: u32) -> [u8; 4] {
        match self {
            Self::Big => value.to_be_bytes(),
            Self::Little => value.to_le_bytes(),
        }
    }
}

/// EXIF holding nothing but the orientation `tiff` records, if it records
/// one other than upright.
pub fn orientation_only(tiff: &[u8]) -> Option<Vec<u8>> {
    let order = ByteOrder::of(tiff)?;
    let u16_at = |at| array(tiff, at).map(|bytes| order.u16(bytes));
    let first_directory = usize::try_from(order.u32(array(tiff, 4)?)).ok()?;
    let entries = u16_at(first_directory)?;
    let orientation = (0..usize::from(entries)).find_map(|index| {
        let entry = first_directory + 2 + 12 * index;
        let is_orientation = u16_at(entry)? == ORIENTATION && u16_at(entry + 2)? == SHORT;
        is_orientation.then(|| u16_at(entry + 8)).flatten()
    })?;
    (2..=8)
        .contains(&orientation)
        .then(|| with_orientation(orientation, order))
}

/// The `N` bytes of `tiff` at `at`, if it's that long.
fn array<const N: usize>(tiff: &[u8], at: usize) -> Option<[u8; N]> {
    tiff.get(at..at.checked_add(N)?)?.try_into().ok()
}

/// A TIFF header and one directory, of one entry: the orientation.
fn with_orientation(orientation: u16, order: ByteOrder) -> Vec<u8> {
    let mut tiff = Vec::with_capacity(ORIENTATION_ONLY_LENGTH);
    tiff.extend(order.header());
    tiff.extend(order.u32_bytes(8));
    tiff.extend(order.u16_bytes(1));
    tiff.extend(order.u16_bytes(ORIENTATION));
    tiff.extend(order.u16_bytes(SHORT));
    tiff.extend(order.u32_bytes(1));
    tiff.extend(order.u16_bytes(orientation));
    tiff.extend([0, 0]);
    tiff.extend(order.u32_bytes(0));
    tiff
}

#[cfg(test)]
pub mod tests {
    use super::*;

    /// The orientation of EXIF that holds nothing else.
    pub fn orientation(tiff: &[u8]) -> Option<u16> {
        let order = ByteOrder::of(tiff)?;
        let kept = (tiff.len() == ORIENTATION_ONLY_LENGTH).then_some(tiff)?;
        Some(order.u16([kept[18], kept[19]]))
    }

    /// Little-endian EXIF of a photo taken on its side, naming the phone
    /// that took it.
    pub fn turned() -> Vec<u8> {
        let order = ByteOrder::Little;
        let make = b"Pixelphone\0";
        let make_at = 8 + 2 + 2 * 12 + 4;
        let mut tiff = order.header().to_vec();
        tiff.extend(order.u32_bytes(8));
        tiff.extend(order.u16_bytes(2));
        tiff.extend(order.u16_bytes(0x010f));
        tiff.extend(order.u16_bytes(2));
        tiff.extend(order.u32_bytes(make.len() as u32));
        tiff.extend(order.u32_bytes(make_at));
        tiff.extend(order.u16_bytes(ORIENTATION));
        tiff.extend(order.u16_bytes(SHORT));
        tiff.extend(order.u32_bytes(1));
        tiff.extend([6, 0, 0, 0]);
        tiff.extend(order.u32_bytes(0));
        tiff.extend(make);
        tiff
    }

    #[test]
    fn keeps_only_the_orientation_of_a_photo_taken_on_its_side() {
        let kept = orientation_only(&turned()).unwrap();
        assert_eq!(kept, with_orientation(6, ByteOrder::Little));
        assert_eq!(orientation(&kept), Some(6));
    }

    #[test]
    fn keeps_a_turned_photos_orientation_in_either_byte_order() {
        for order in [ByteOrder::Big, ByteOrder::Little] {
            let tiff = with_orientation(6, order);
            assert_eq!(tiff.len(), ORIENTATION_ONLY_LENGTH);
            assert_eq!(orientation_only(&tiff), Some(tiff.clone()));
            assert_eq!(orientation(&tiff), Some(6));
        }
    }

    #[test]
    fn keeps_nothing_of_an_upright_photo_or_of_exif_without_an_orientation() {
        assert_eq!(orientation_only(&with_orientation(1, ByteOrder::Big)), None);
        let mut make = with_orientation(6, ByteOrder::Little);
        make[10] = 0x0f;
        assert_eq!(orientation_only(&make), None);
    }

    #[test]
    fn makes_nothing_of_exif_that_is_cut_short_or_points_outside_itself() {
        let tiff = with_orientation(6, ByteOrder::Big);
        for cut in 0..20 {
            assert_eq!(orientation_only(&tiff[..cut]), None, "{cut}");
        }
        let mut far = tiff.clone();
        far[4..8].copy_from_slice(&u32::MAX.to_be_bytes());
        assert_eq!(orientation_only(&far), None);
        let mut many = tiff.clone();
        many[8..10].copy_from_slice(&u16::MAX.to_be_bytes());
        assert_eq!(orientation_only(&many), Some(tiff));
        assert_eq!(orientation_only(b"not exif at all"), None);
    }
}
