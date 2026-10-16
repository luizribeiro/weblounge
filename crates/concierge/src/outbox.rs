//! What concierge leaves in the outbox for concierge-mailer, and when the
//! mailer tries a message again after failing to send it.
//!
//! Times are Unix seconds.

use std::fmt;

use crate::email::Email;

const MINUTE: i64 = 60;
const HOUR: i64 = 60 * MINUTE;
const DAY: i64 = 24 * HOUR;

/// The wait after the first failed attempt, doubling with each failure
/// after it.
pub const FIRST_RETRY: i64 = MINUTE;
/// The longest wait between attempts.
pub const MAX_RETRY: i64 = HOUR;
/// How long a message waits to be sent before the mailer gives up on it.
/// Verification links last this long, so a later message would carry a
/// dead link; and an address that can't be reached in two days is not
/// worth keeping on the disk.
pub const GIVE_UP_AFTER: i64 = 2 * DAY;

/// When to try a message again after its `attempts`th failure at `now`.
pub fn next_attempt_at(attempts: i64, now: i64) -> i64 {
    let doublings = attempts.saturating_sub(1).clamp(0, u32::MAX.into()) as u32;
    let wait = 2_i64.saturating_pow(doublings).saturating_mul(FIRST_RETRY);
    now + wait.min(MAX_RETRY)
}

/// The mailer has given up by `now` on the messages queued before this.
pub fn given_up_before(now: i64) -> i64 {
    now - GIVE_UP_AFTER
}

/// A subject line, which can't break out of its header.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Subject(String);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InvalidSubject;

impl fmt::Display for InvalidSubject {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("A subject is one line without control characters.")
    }
}

impl std::error::Error for InvalidSubject {}

impl Subject {
    pub fn parse(subject: &str) -> Result<Self, InvalidSubject> {
        if subject.chars().any(char::is_control) {
            return Err(InvalidSubject);
        }
        Ok(Self(subject.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A plain-text message to queue.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    /// The member it's for, if any, whose removal deletes it.
    pub account: Option<String>,
    pub to: Email,
    pub subject: Subject,
    pub body: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: i64 = 1_760_000_000;

    #[test]
    fn waits_double_from_a_minute_up_to_an_hour() {
        let waits: Vec<_> = (1..=9)
            .map(|attempts| next_attempt_at(attempts, NOW) - NOW)
            .collect();
        assert_eq!(
            waits,
            [1, 2, 4, 8, 16, 32, 60, 60, 60].map(|minutes| minutes * MINUTE)
        );
        assert_eq!(next_attempt_at(i64::MAX, NOW), NOW + MAX_RETRY);
    }

    #[test]
    fn gives_up_two_days_after_queueing() {
        assert_eq!(given_up_before(NOW + 2 * DAY), NOW);
    }

    #[test]
    fn a_subject_is_one_line() {
        let subject = "Verify your email for weblounge";
        assert_eq!(Subject::parse(subject).unwrap().as_str(), subject);
        for subject in [
            "Verify\r\nBcc: mallory@example.org",
            "Verify\nyour email",
            "Verify\ryour email",
            "Verify\0",
        ] {
            assert_eq!(Subject::parse(subject), Err(InvalidSubject), "{subject:?}");
        }
    }
}
