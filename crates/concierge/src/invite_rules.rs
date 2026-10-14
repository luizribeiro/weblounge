//! Who may invite, how many, and how long an invite stays open. Members
//! invite from the day they join, up to a quota.
//!
//! Times are Unix seconds.

const DAY: i64 = 24 * 60 * 60;

/// How many people a member can bring in: open invites and redeemed ones
/// count, expired and revoked ones don't. Admins can raise it per member.
pub const DEFAULT_QUOTA: i64 = 5;
/// How long an invite link stays open.
pub const LIFETIME: i64 = 7 * DAY;
/// How long a redemption holds its invite. Far longer than a redemption
/// takes (Ergo's API times out after 10 seconds), so an older claim was
/// abandoned by a process that died midway.
pub const CLAIM_TIMEOUT: i64 = 2 * 60;

pub fn expires_at(created_at: i64) -> i64 {
    created_at + LIFETIME
}

/// The member who wants to invite someone.
pub struct Inviter {
    pub invite_quota: i64,
    pub is_admin: bool,
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
    QuotaUsed { quota: i64 },
}

/// Whether `inviter`, who created `invites`, may create another one now.
/// Admins are exempt from the quota.
pub fn may_invite<'a>(
    inviter: &Inviter,
    invites: impl IntoIterator<Item = &'a Invite>,
    now: i64,
) -> Result<(), Refusal> {
    if inviter.removed {
        return Err(Refusal::Removed);
    }
    if inviter.is_admin {
        return Ok(());
    }
    let used = invites
        .into_iter()
        .filter(|invite| !matches!(invite.status(now), Status::Expired | Status::Revoked))
        .count();
    if used as i64 >= inviter.invite_quota {
        return Err(Refusal::QuotaUsed {
            quota: inviter.invite_quota,
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const JOINED: i64 = 1_760_000_000;

    fn member() -> Inviter {
        Inviter {
            invite_quota: DEFAULT_QUOTA,
            is_admin: false,
            removed: false,
        }
    }

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
            redeemed_at: Some(created_at + DAY),
            ..open(created_at)
        }
    }

    fn revoked(created_at: i64) -> Invite {
        Invite {
            revoked_at: Some(created_at + DAY),
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
            claimed_at: Some(JOINED + DAY),
            ..open(JOINED)
        };
        assert_eq!(claimed.status(JOINED + DAY), Status::Claimed);
        assert_eq!(
            claimed.status(JOINED + DAY + CLAIM_TIMEOUT - 1),
            Status::Claimed
        );
        assert_eq!(claimed.status(JOINED + DAY + CLAIM_TIMEOUT), Status::Open);
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
        assert_eq!(may_invite(&member(), [], JOINED), Ok(()));
    }

    #[test]
    fn open_claimed_and_redeemed_invites_use_up_the_quota() {
        let now = JOINED + 10 * DAY;
        let invites = [
            redeemed(JOINED),
            redeemed(JOINED),
            open(now - DAY),
            open(now - DAY),
        ];
        assert_eq!(may_invite(&member(), &invites, now), Ok(()));
        let claimed = Invite {
            claimed_at: Some(now),
            ..open(now)
        };
        let invites: Vec<_> = invites.into_iter().chain([claimed]).collect();
        assert_eq!(
            may_invite(&member(), &invites, now),
            Err(Refusal::QuotaUsed {
                quota: DEFAULT_QUOTA
            })
        );
    }

    #[test]
    fn expired_and_revoked_invites_give_their_place_back() {
        let now = JOINED + 10 * DAY;
        let invites = [
            open(JOINED),
            revoked(now - DAY),
            open(JOINED + DAY),
            revoked(now - DAY),
            revoked(now - DAY),
        ];
        assert_eq!(may_invite(&member(), &invites, now), Ok(()));
    }

    #[test]
    fn a_raised_quota_allows_more() {
        let inviter = Inviter {
            invite_quota: 6,
            ..member()
        };
        let invites: Vec<_> = (0..5).map(|_| redeemed(JOINED)).collect();
        assert_eq!(may_invite(&inviter, &invites, JOINED + DAY), Ok(()));
    }

    #[test]
    fn admins_invite_without_a_quota() {
        let admin = Inviter {
            is_admin: true,
            invite_quota: 0,
            ..member()
        };
        let invites: Vec<_> = (0..20).map(|_| open(JOINED)).collect();
        assert_eq!(may_invite(&admin, &invites, JOINED), Ok(()));
    }

    #[test]
    fn removed_members_invite_nobody() {
        let removed = Inviter {
            removed: true,
            is_admin: true,
            ..member()
        };
        assert_eq!(
            may_invite(&removed, [], JOINED + DAY),
            Err(Refusal::Removed)
        );
    }
}
