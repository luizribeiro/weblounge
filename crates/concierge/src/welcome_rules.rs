//! When the greeter welcomes a newcomer, tells their inviter and gives up.
//! Everything starts at first sight, the first time the newcomer is seen
//! with a session attached, so nobody is welcomed while they're away; an
//! inviter whose invitee hasn't connected two days in hears so once.
//!
//! Times are Unix seconds.

const HOUR: i64 = 60 * 60;
const DAY: i64 = 24 * HOUR;

/// How long a direct message the server refused, to someone not
/// connected, is tried again.
pub const RETRY_FOR: i64 = DAY;
/// How long after joining an inviter hears that their invitee hasn't
/// connected.
pub const NUDGE_AFTER: i64 = 2 * DAY;
/// How long the record of a newcomer's welcome is kept, and so how long
/// a first sight is still welcomed.
pub const KEPT: i64 = 14 * DAY;

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
