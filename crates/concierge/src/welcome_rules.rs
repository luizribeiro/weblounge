//! When the greeter welcomes a newcomer, tells their inviter and gives up.
//! The welcome goes out when the newcomer is first seen with a session
//! attached, or a few minutes after they join, whichever is first; a
//! welcome the greeter couldn't send in time is skipped, never sent late.
//!
//! Times are Unix seconds.

const MINUTE: i64 = 60;
const HOUR: i64 = 60 * MINUTE;
const DAY: i64 = 24 * HOUR;

/// How long after joining the welcome goes out without the newcomer.
pub const FALLBACK: i64 = 10 * MINUTE;
/// How old an arrival may be and still be welcomed and announced to its
/// inviter.
pub const STALE_AFTER: i64 = 2 * DAY;
/// How long a direct message the server refused, to someone not
/// connected, is tried again.
pub const RETRY_FOR: i64 = DAY;
/// How long after joining an inviter hears that their invitee hasn't
/// connected.
pub const NUDGE_AFTER: i64 = 2 * DAY;
/// How long the record of a newcomer's welcome is kept.
pub const KEPT: i64 = 14 * DAY;

/// Whether a newcomer who joined at `joined_at` is too old to welcome.
pub fn stale(joined_at: i64, now: i64) -> bool {
    now >= joined_at + STALE_AFTER
}

/// Whether the welcome of a newcomer who joined at `joined_at` is due,
/// given whether they were seen connected.
pub fn welcome_due(joined_at: i64, seen: bool, now: i64) -> bool {
    !stale(joined_at, now) && (seen || now >= joined_at + FALLBACK)
}

/// Whether a direct message first due at `due_at` is still worth trying.
pub fn still_trying(due_at: i64, now: i64) -> bool {
    now < due_at + RETRY_FOR
}

/// When the inviter of a newcomer who joined at `joined_at`, and hasn't
/// connected since, is nudged.
pub fn nudge_at(joined_at: i64) -> i64 {
    joined_at + NUDGE_AFTER
}

/// The records of newcomers who joined at or before this are past keeping
/// at `now`.
pub fn forgotten_by(now: i64) -> i64 {
    now - KEPT
}

#[cfg(test)]
mod tests {
    use super::*;

    const JOINED: i64 = 1_760_000_000;

    #[test]
    fn the_welcome_waits_for_the_newcomer_or_the_fallback() {
        assert!(!welcome_due(JOINED, false, JOINED));
        assert!(!welcome_due(JOINED, false, JOINED + FALLBACK - 1));
        assert!(welcome_due(JOINED, false, JOINED + FALLBACK));
        assert!(welcome_due(JOINED, true, JOINED));
    }

    #[test]
    fn a_welcome_two_days_late_is_skipped_even_with_the_newcomer_there() {
        assert!(welcome_due(JOINED, true, JOINED + STALE_AFTER - 1));
        assert!(!welcome_due(JOINED, true, JOINED + STALE_AFTER));
        assert!(!welcome_due(JOINED, false, JOINED + STALE_AFTER));
        assert!(stale(JOINED, JOINED + 3 * DAY));
    }

    #[test]
    fn a_refused_message_is_tried_for_a_day() {
        assert!(still_trying(JOINED, JOINED));
        assert!(still_trying(JOINED, JOINED + DAY - 1));
        assert!(!still_trying(JOINED, JOINED + DAY));
    }

    #[test]
    fn the_nudge_comes_two_days_after_joining() {
        assert_eq!(nudge_at(JOINED), JOINED + 2 * DAY);
    }

    #[test]
    fn the_record_is_kept_two_weeks() {
        assert_eq!(forgotten_by(JOINED + 14 * DAY - 1), JOINED - 1);
        assert_eq!(forgotten_by(JOINED + 14 * DAY), JOINED);
    }
}
