//! Who may invite, how many, and how long an invite stays open. The
//! defaults follow lobste.rs: new members wait before they can invite.
//!
//! Times are Unix seconds.

const DAY: i64 = 24 * 60 * 60;

/// How long a new member waits before they can invite anyone.
pub const WAITING_PERIOD: i64 = 70 * DAY;
/// How many people a member can bring in: open invites and redeemed ones
/// count, expired and revoked ones don't. Admins can raise it per member.
pub const DEFAULT_QUOTA: i64 = 5;
/// How long an invite link stays open.
pub const LIFETIME: i64 = 7 * DAY;

pub fn expires_at(created_at: i64) -> i64 {
    created_at + LIFETIME
}

/// The member who wants to invite someone.
pub struct Inviter {
    pub joined_at: i64,
    pub invite_quota: i64,
    pub is_admin: bool,
    pub removed: bool,
}

pub struct Invite {
    pub expires_at: i64,
    pub redeemed_at: Option<i64>,
    pub revoked_at: Option<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Open,
    Redeemed,
    Revoked,
    Expired,
}

impl Invite {
    pub fn status(&self, now: i64) -> Status {
        if self.redeemed_at.is_some() {
            Status::Redeemed
        } else if self.revoked_at.is_some() {
            Status::Revoked
        } else if now >= self.expires_at {
            Status::Expired
        } else {
            Status::Open
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    Removed,
    TooNew { until: i64 },
    QuotaUsed { quota: i64 },
}

/// Whether `inviter`, who created `invites`, may create another one now.
/// Admins are exempt from the waiting period and the quota.
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
    let until = inviter.joined_at + WAITING_PERIOD;
    if now < until {
        return Err(Refusal::TooNew { until });
    }
    let used = invites
        .into_iter()
        .filter(|invite| matches!(invite.status(now), Status::Open | Status::Redeemed))
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
    const SETTLED: i64 = JOINED + WAITING_PERIOD;

    fn member() -> Inviter {
        Inviter {
            joined_at: JOINED,
            invite_quota: DEFAULT_QUOTA,
            is_admin: false,
            removed: false,
        }
    }

    fn open(created_at: i64) -> Invite {
        Invite {
            expires_at: expires_at(created_at),
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
        let invite = open(SETTLED);
        assert_eq!(invite.status(SETTLED), Status::Open);
        assert_eq!(invite.status(SETTLED + LIFETIME - 1), Status::Open);
        assert_eq!(invite.status(SETTLED + LIFETIME), Status::Expired);
    }

    #[test]
    fn redeemed_and_revoked_invites_stay_so_after_expiring() {
        let later = SETTLED + 2 * LIFETIME;
        assert_eq!(redeemed(SETTLED).status(later), Status::Redeemed);
        assert_eq!(revoked(SETTLED).status(later), Status::Revoked);
    }

    #[test]
    fn new_members_wait_before_inviting() {
        assert_eq!(
            may_invite(&member(), [], SETTLED - 1),
            Err(Refusal::TooNew { until: SETTLED })
        );
        assert_eq!(may_invite(&member(), [], SETTLED), Ok(()));
    }

    #[test]
    fn open_and_redeemed_invites_use_up_the_quota() {
        let now = SETTLED + 10 * DAY;
        let invites = [
            redeemed(SETTLED),
            redeemed(SETTLED),
            open(now - DAY),
            open(now - DAY),
        ];
        assert_eq!(may_invite(&member(), &invites, now), Ok(()));
        let invites: Vec<_> = invites.into_iter().chain([open(now)]).collect();
        assert_eq!(
            may_invite(&member(), &invites, now),
            Err(Refusal::QuotaUsed {
                quota: DEFAULT_QUOTA
            })
        );
    }

    #[test]
    fn expired_and_revoked_invites_give_their_place_back() {
        let now = SETTLED + 10 * DAY;
        let invites = [
            open(SETTLED),
            revoked(now - DAY),
            open(SETTLED + DAY),
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
        let invites: Vec<_> = (0..5).map(|_| redeemed(SETTLED)).collect();
        assert_eq!(may_invite(&inviter, &invites, SETTLED + DAY), Ok(()));
    }

    #[test]
    fn admins_invite_from_day_one_without_a_quota() {
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
            may_invite(&removed, [], SETTLED + DAY),
            Err(Refusal::Removed)
        );
    }
}
