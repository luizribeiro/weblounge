//! How much members may upload, and for how long uploads stay. A bot's
//! uploads count against its owner's room, but against its own hourly
//! limit, so a busy bot doesn't stop its owner from pasting a screenshot.
//!
//! Times are Unix seconds.

use filehost::ImageType;

const DAY: i64 = 24 * 60 * 60;
const MEGABYTE: i64 = 1_000_000;

/// The largest image anyone may upload, in bytes.
pub const MAX_BYTES: i64 = filehost::MAX_BYTES as i64;
/// The bytes a member's uploads, their bots' included, may take at once.
pub const QUOTA_BYTES: i64 = 500 * MEGABYTE;
/// Uploads an account may make an hour.
pub const PER_HOUR: usize = 30;
/// How long an upload stays before it's deleted.
pub const RETENTION: i64 = 30 * DAY;

/// The limits as members read them.
pub struct InWords {
    pub quota: String,
    pub max: String,
    pub per_hour: usize,
    pub kept: String,
    pub types: String,
}

impl Default for InWords {
    fn default() -> Self {
        Self {
            quota: in_megabytes(QUOTA_BYTES),
            max: in_megabytes(MAX_BYTES),
            per_hour: PER_HOUR,
            kept: crate::invite_rules::in_words(RETENTION),
            types: types_in_words(),
        }
    }
}

/// The types the network takes, as help text lists them.
pub fn types_in_words() -> String {
    let labels = ImageType::ALL.map(ImageType::label);
    let (last, rest) = labels.split_last().expect("there are types");
    format!("{} or {last}", rest.join(", "))
}

/// Whether an upload of `size` bytes fits beside the `used` bytes its
/// owner's uploads take.
pub fn fits(used: i64, size: i64) -> bool {
    size <= MAX_BYTES && used + size <= QUOTA_BYTES
}

/// When an upload made at `created_at` is deleted.
pub fn expires_at(created_at: i64) -> i64 {
    created_at + RETENTION
}

/// The uploads made at or before this have expired by `now`.
pub fn expired_by(now: i64) -> i64 {
    now - RETENTION
}

/// Whole days, rounded up, until an upload made at `created_at` is
/// deleted, as of `now`: at least one, since the hourly expiry may not
/// have got to it yet.
pub fn days_left(created_at: i64, now: i64) -> i64 {
    let left = expires_at(created_at) - now;
    rounded_up(left.max(1), DAY)
}

/// `bytes` in megabytes, as the Uploads tab shows them: a decimal under
/// ten, so a screenshot doesn't show as nothing.
pub fn in_megabytes(bytes: i64) -> String {
    if bytes == 0 {
        return "0 MB".into();
    }
    let tenths = rounded_up(bytes * 10, MEGABYTE);
    if tenths < 100 {
        format!("{}.{} MB", tenths / 10, tenths % 10)
    } else {
        format!("{} MB", rounded_up(bytes, MEGABYTE))
    }
}

/// `amount` in units of `unit`, rounded up; both are positive.
fn rounded_up(amount: i64, unit: i64) -> i64 {
    (amount + unit - 1) / unit
}

#[cfg(test)]
mod tests {
    use super::*;

    const CREATED: i64 = 1_760_000_000;

    #[test]
    fn an_upload_fits_while_it_is_small_enough_and_there_is_room() {
        assert!(fits(0, MAX_BYTES));
        assert!(!fits(0, MAX_BYTES + 1));
        assert!(fits(QUOTA_BYTES - 1_000, 1_000));
        assert!(!fits(QUOTA_BYTES - 1_000, 1_001));
        assert!(!fits(QUOTA_BYTES, 1));
    }

    #[test]
    fn names_the_types_the_network_takes() {
        assert_eq!(types_in_words(), "PNG, JPEG, GIF or WebP");
    }

    #[test]
    fn uploads_stay_thirty_days() {
        assert_eq!(expires_at(CREATED), CREATED + 30 * DAY);
        assert_eq!(expired_by(CREATED + 30 * DAY), CREATED);
    }

    #[test]
    fn counts_the_days_left_rounded_up_and_never_below_one() {
        assert_eq!(days_left(CREATED, CREATED), 30);
        assert_eq!(days_left(CREATED, CREATED + 1), 30);
        assert_eq!(days_left(CREATED, CREATED + DAY), 29);
        assert_eq!(days_left(CREATED, CREATED + 29 * DAY + 1), 1);
        assert_eq!(days_left(CREATED, CREATED + 31 * DAY), 1);
    }

    #[test]
    fn shows_megabytes_with_a_decimal_under_ten() {
        assert_eq!(in_megabytes(0), "0 MB");
        assert_eq!(in_megabytes(1), "0.1 MB");
        assert_eq!(in_megabytes(80_000), "0.1 MB");
        assert_eq!(in_megabytes(2_500_000), "2.5 MB");
        assert_eq!(in_megabytes(9_999_999), "10 MB");
        assert_eq!(in_megabytes(10_000_000), "10 MB");
        assert_eq!(in_megabytes(123_400_000), "124 MB");
        assert_eq!(in_megabytes(QUOTA_BYTES), "500 MB");
    }
}
