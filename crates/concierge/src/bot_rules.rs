//! Who may make bots on the site, and how many. Members make bots from the
//! day they confirm their email, as they invite, a few live ones at a
//! time; deleted bots free their place. Admins make more from the command
//! line.

/// How many live bots a member may have, counting the ones an admin made
/// for them.
pub const MAX_LIVE: i64 = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    Unverified,
    TooMany,
}

/// How many more bots a member who `verified` their email and has `live`
/// bots may make now.
pub fn remaining(verified: bool, live: i64) -> i64 {
    if !verified {
        return 0;
    }
    (MAX_LIVE - live).max(0)
}

/// Whether a member who `verified` their email and has `live` bots may make
/// another now.
pub fn may_create(verified: bool, live: i64) -> Result<(), Refusal> {
    if !verified {
        Err(Refusal::Unverified)
    } else if remaining(verified, live) == 0 {
        Err(Refusal::TooMany)
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn confirmed_members_make_up_to_three_live_bots() {
        assert_eq!(remaining(true, 0), MAX_LIVE);
        assert_eq!(may_create(true, 0), Ok(()));
        assert_eq!(remaining(true, MAX_LIVE - 1), 1);
        assert_eq!(may_create(true, MAX_LIVE - 1), Ok(()));
        assert_eq!(remaining(true, MAX_LIVE), 0);
        assert_eq!(may_create(true, MAX_LIVE), Err(Refusal::TooMany));
    }

    #[test]
    fn bots_an_admin_made_beyond_the_cap_leave_none_to_make() {
        assert_eq!(remaining(true, MAX_LIVE + 2), 0);
        assert_eq!(may_create(true, MAX_LIVE + 2), Err(Refusal::TooMany));
    }

    #[test]
    fn members_who_havent_confirmed_their_email_make_none() {
        assert_eq!(remaining(false, 0), 0);
        assert_eq!(may_create(false, 0), Err(Refusal::Unverified));
        assert_eq!(may_create(false, MAX_LIVE), Err(Refusal::Unverified));
    }
}
