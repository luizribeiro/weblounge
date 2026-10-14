//! Who may invite, how many, and how long an invite stays open. Members
//! invite from the day they join, a few open invites at a time.
//!
//! Times are Unix seconds.

const HOUR: i64 = 60 * 60;
const DAY: i64 = 24 * HOUR;

/// How many invites a member, admins too, may have open at once. Nothing
/// lists or revokes open invites, so each holds its place until it's
/// redeemed or expires.
pub const MAX_OPEN: i64 = 3;
/// How long an invite link stays open.
pub const LIFETIME: i64 = DAY;
/// How long a redemption holds its invite. Far longer than a redemption
/// takes (Ergo's API times out after 10 seconds), so an older claim was
/// abandoned by a process that died midway.
pub const CLAIM_TIMEOUT: i64 = 2 * 60;

pub fn expires_at(created_at: i64) -> i64 {
    created_at + LIFETIME
}

/// The member who wants to invite someone.
pub struct Inviter {
    pub removed: bool,
}

pub struct Invite {
    pub expires_at: i64,
    pub claimed_at: Option<i64>,
    pub redeemed_at: Option<i64>,
    pub revoked_at: Option<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Open,
    Redeemed,
    Revoked,
    Expired,
    /// Someone is redeeming it right now.
    Claimed,
}

impl Invite {
    pub fn status(&self, now: i64) -> Status {
        if self.redeemed_at.is_some() {
            Status::Redeemed
        } else if self.revoked_at.is_some() {
            Status::Revoked
        } else if now >= self.expires_at {
            Status::Expired
        } else if self
            .claimed_at
            .is_some_and(|claimed_at| now < claimed_at + CLAIM_TIMEOUT)
        {
            Status::Claimed
        } else {
            Status::Open
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    Removed,
    TooManyOpen,
}

/// How many more invites `inviter`, who created `invites`, may create now.
pub fn remaining<'a>(
    inviter: &Inviter,
    invites: impl IntoIterator<Item = &'a Invite>,
    now: i64,
) -> i64 {
    if inviter.removed {
        return 0;
    }
    let open = invites
        .into_iter()
        .filter(|invite| matches!(invite.status(now), Status::Open | Status::Claimed))
        .count();
    (MAX_OPEN - open as i64).max(0)
}

/// Whether `inviter`, who created `invites`, may create another one now.
pub fn may_invite<'a>(
    inviter: &Inviter,
    invites: impl IntoIterator<Item = &'a Invite>,
    now: i64,
) -> Result<(), Refusal> {
    if inviter.removed {
        return Err(Refusal::Removed);
    }
    match remaining(inviter, invites, now) {
        0 => Err(Refusal::TooManyOpen),
        _ => Ok(()),
    }
}

/// A duration of `seconds`, in words: "6 days", "24 hours", "1 hour".
pub fn in_words(seconds: i64) -> String {
    let (n, unit) = if seconds >= 2 * DAY {
        (seconds / DAY, "day")
    } else {
        ((seconds / HOUR).max(1), "hour")
    };
    let plural = if n == 1 { "" } else { "s" };
    format!("{n} {unit}{plural}")
}

#[cfg(test)]
mod tests {
    use super::*;

    const JOINED: i64 = 1_760_000_000;
    const MEMBER: Inviter = Inviter { removed: false };

    fn open(created_at: i64) -> Invite {
        Invite {
            expires_at: expires_at(created_at),
            claimed_at: None,
            redeemed_at: None,
            revoked_at: None,
        }
    }

    fn redeemed(created_at: i64) -> Invite {
        Invite {
            redeemed_at: Some(created_at + HOUR),
            ..open(created_at)
        }
    }

    fn revoked(created_at: i64) -> Invite {
        Invite {
            revoked_at: Some(created_at + HOUR),
            ..open(created_at)
        }
    }

    fn claimed(created_at: i64) -> Invite {
        Invite {
            claimed_at: Some(created_at),
            ..open(created_at)
        }
    }

    #[test]
    fn an_invite_is_open_until_it_expires() {
        let invite = open(JOINED);
        assert_eq!(invite.status(JOINED), Status::Open);
        assert_eq!(invite.status(JOINED + LIFETIME - 1), Status::Open);
        assert_eq!(invite.status(JOINED + LIFETIME), Status::Expired);
    }

    #[test]
    fn a_claim_holds_the_invite_until_it_goes_stale() {
        let claimed = Invite {
            claimed_at: Some(JOINED + HOUR),
            ..open(JOINED)
        };
        assert_eq!(claimed.status(JOINED + HOUR), Status::Claimed);
        assert_eq!(
            claimed.status(JOINED + HOUR + CLAIM_TIMEOUT - 1),
            Status::Claimed
        );
        assert_eq!(claimed.status(JOINED + HOUR + CLAIM_TIMEOUT), Status::Open);
        assert_eq!(claimed.status(JOINED + LIFETIME), Status::Expired);
    }

    #[test]
    fn redeemed_and_revoked_invites_stay_so_after_expiring() {
        let later = JOINED + 2 * LIFETIME;
        assert_eq!(redeemed(JOINED).status(later), Status::Redeemed);
        assert_eq!(revoked(JOINED).status(later), Status::Revoked);
    }

    #[test]
    fn members_invite_from_the_day_they_join() {
        assert_eq!(may_invite(&MEMBER, [], JOINED), Ok(()));
        assert_eq!(remaining(&MEMBER, [], JOINED), MAX_OPEN);
    }

    #[test]
    fn open_and_claimed_invites_take_the_places() {
        let now = JOINED + HOUR;
        let invites = [open(JOINED), claimed(now)];
        assert_eq!(remaining(&MEMBER, &invites, now), 1);
        assert_eq!(may_invite(&MEMBER, &invites, now), Ok(()));
        let invites: Vec<_> = invites.into_iter().chain([open(now)]).collect();
        assert_eq!(remaining(&MEMBER, &invites, now), 0);
        assert_eq!(
            may_invite(&MEMBER, &invites, now),
            Err(Refusal::TooManyOpen)
        );
    }

    #[test]
    fn redeemed_expired_and_revoked_invites_free_their_place() {
        let now = JOINED + 2 * LIFETIME;
        let invites: Vec<_> = (0..10)
            .flat_map(|_| [redeemed(now - HOUR), open(JOINED), revoked(now - HOUR)])
            .collect();
        assert_eq!(remaining(&MEMBER, &invites, now), MAX_OPEN);
    }

    #[test]
    fn says_durations_in_words() {
        assert_eq!(in_words(LIFETIME), "24 hours");
        assert_eq!(in_words(7 * DAY + HOUR), "7 days");
        assert_eq!(in_words(DAY + HOUR), "25 hours");
        assert_eq!(in_words(5 * HOUR + 59), "5 hours");
        assert_eq!(in_words(60), "1 hour");
    }

    #[test]
    fn removed_members_invite_nobody() {
        let removed = Inviter { removed: true };
        assert_eq!(may_invite(&removed, [], JOINED), Err(Refusal::Removed));
        assert_eq!(remaining(&removed, [], JOINED), 0);
    }
}
