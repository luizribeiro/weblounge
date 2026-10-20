//! How members verify their email: each link lasts two days, and a member
//! has a week from the first one, with one reminder on the way. IRC works
//! without a verified email; inviting needs one.
//!
//! Times are Unix seconds.

const HOUR: i64 = 60 * 60;
const DAY: i64 = 24 * HOUR;

/// How long a verification link lasts. The mailer gives up on the email
/// that carries it once it stops working.
pub const LIFETIME: i64 = 2 * DAY;
/// How long a member has to verify, from the first verification email.
pub const GRACE_PERIOD: i64 = 7 * DAY;
/// When the one reminder goes out, after the first verification email.
pub const REMINDER_AFTER: i64 = 5 * DAY;

pub fn expires_at(created_at: i64) -> i64 {
    created_at + LIFETIME
}

/// When the grace period of a member first mailed at `first_sent_at` ends.
pub fn deadline(first_sent_at: i64) -> i64 {
    first_sent_at + GRACE_PERIOD
}

pub struct Code {
    pub expires_at: i64,
    /// When a newer code for the same member replaced it.
    pub replaced_at: Option<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Live,
    Expired,
    /// A newer email went out, whose link is the one that works.
    Replaced,
}

impl Code {
    pub fn status(&self, now: i64) -> Status {
        if self.replaced_at.is_some() {
            Status::Replaced
        } else if now >= self.expires_at {
            Status::Expired
        } else {
            Status::Live
        }
    }
}

/// How far a member got with verifying their email.
pub struct Progress {
    pub verified: bool,
    /// When the first verification email went out, if one has.
    pub first_sent_at: Option<i64>,
    pub reminded_at: Option<i64>,
    /// When the newest verification email went out, while its link is on
    /// file.
    pub last_sent_at: Option<i64>,
}

impl Progress {
    /// When the member's grace period ends, once it has started.
    pub fn deadline(&self) -> Option<i64> {
        self.first_sent_at.map(deadline)
    }

    /// Whether the member should get their reminder now. One whose last
    /// link still works waits for it to lapse, so nobody gets two links in
    /// a row.
    pub fn reminder_due(&self, now: i64) -> bool {
        let Some(first_sent_at) = self.first_sent_at else {
            return false;
        };
        let link_still_works = self
            .last_sent_at
            .is_some_and(|sent_at| now < expires_at(sent_at));
        !self.verified
            && self.reminded_at.is_none()
            && now >= first_sent_at + REMINDER_AFTER
            && !link_still_works
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tree;

    const SENT: i64 = 1_760_000_000;

    fn live(created_at: i64) -> Code {
        Code {
            expires_at: expires_at(created_at),
            replaced_at: None,
        }
    }

    /// A member mailed once, at SENT, who hasn't verified.
    fn mailed_once() -> Progress {
        Progress {
            verified: false,
            first_sent_at: Some(SENT),
            reminded_at: None,
            last_sent_at: Some(SENT),
        }
    }

    #[test]
    fn a_link_lasts_two_days() {
        assert_eq!(LIFETIME, 2 * DAY);
    }

    #[test]
    fn a_code_works_until_it_expires() {
        let code = live(SENT);
        assert_eq!(code.status(SENT), Status::Live);
        assert_eq!(code.status(SENT + LIFETIME - 1), Status::Live);
        assert_eq!(code.status(SENT + LIFETIME), Status::Expired);
    }

    #[test]
    fn a_replaced_code_says_so_even_once_expired() {
        let code = Code {
            replaced_at: Some(SENT + HOUR),
            ..live(SENT)
        };
        assert_eq!(code.status(SENT + HOUR), Status::Replaced);
        assert_eq!(code.status(SENT + 2 * LIFETIME), Status::Replaced);
    }

    #[test]
    fn the_grace_period_is_a_week_from_the_first_email() {
        assert_eq!(deadline(SENT), SENT + 7 * DAY);
        let resent = Progress {
            last_sent_at: Some(SENT + 3 * DAY),
            ..mailed_once()
        };
        assert_eq!(resent.deadline(), Some(SENT + 7 * DAY));
        let never_mailed = Progress {
            first_sent_at: None,
            last_sent_at: None,
            ..mailed_once()
        };
        assert_eq!(never_mailed.deadline(), None);
    }

    #[test]
    fn deadlines_read_as_a_day_and_a_month_without_the_year() {
        assert_eq!(tree::day_in_words(deadline(SENT)), "16 October");
        let november_3 = 1_793_664_000;
        assert_eq!(tree::day_in_words(november_3), "3 November");
        assert_eq!(tree::day_in_words(november_3 - 1), "2 November");
        let new_years_eve = 1_798_675_200;
        assert_eq!(tree::day_in_words(deadline(new_years_eve)), "7 January");
    }

    #[test]
    fn the_reminder_is_due_on_day_five() {
        let member = mailed_once();
        assert!(!member.reminder_due(SENT + REMINDER_AFTER - 1));
        assert!(member.reminder_due(SENT + REMINDER_AFTER));
        assert!(member.reminder_due(SENT + GRACE_PERIOD + DAY));
    }

    #[test]
    fn nobody_verified_reminded_or_never_mailed_gets_a_reminder() {
        let now = SENT + REMINDER_AFTER;
        for member in [
            Progress {
                verified: true,
                ..mailed_once()
            },
            Progress {
                reminded_at: Some(now - HOUR),
                ..mailed_once()
            },
            Progress {
                first_sent_at: None,
                last_sent_at: None,
                ..mailed_once()
            },
        ] {
            assert!(!member.reminder_due(now));
        }
    }

    #[test]
    fn a_link_asked_for_lately_puts_the_reminder_off_until_it_lapses() {
        let asked_at = SENT + 4 * DAY;
        let member = Progress {
            last_sent_at: Some(asked_at),
            ..mailed_once()
        };
        assert!(!member.reminder_due(SENT + REMINDER_AFTER));
        assert!(!member.reminder_due(expires_at(asked_at) - 1));
        assert!(member.reminder_due(expires_at(asked_at)));
    }
}
