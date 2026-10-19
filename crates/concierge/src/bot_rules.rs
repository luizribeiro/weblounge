//! Who may make bots on the site, and how many. Members make bots from the
//! day they confirm their email, as they invite, a few live ones at a
//! time; deleted bots free their place. Admins make more from the command
//! line. A deleted bot's name is its owner's to reuse, and everyone's once
//! the chat history under it has expired.
//!
//! Times are Unix seconds.

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

/// Whether `claimant` may take the name of a bot `owner` deleted at
/// `removed_at`, at `now`: a member making a bot, or None for someone
/// signing up. Its owner may at once. Anyone else waits until the server's
/// chat history, kept `history_expiry`, no longer holds what the bot said,
/// so nobody new passes for it there.
pub fn deleted_name_free(
    owner: &str,
    removed_at: i64,
    claimant: Option<&str>,
    now: i64,
    history_expiry: i64,
) -> bool {
    claimant == Some(owner) || now >= removed_at + history_expiry
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

    const DELETED: i64 = 1_760_000_000;
    const TWO_WEEKS: i64 = 14 * 24 * 60 * 60;

    #[test]
    fn a_deleted_bots_owner_reuses_its_name_at_once() {
        assert!(deleted_name_free(
            "zoe",
            DELETED,
            Some("zoe"),
            DELETED,
            TWO_WEEKS
        ));
        assert!(deleted_name_free(
            "zoe",
            DELETED,
            Some("zoe"),
            DELETED + 60,
            TWO_WEEKS
        ));
    }

    #[test]
    fn others_take_a_deleted_bots_name_once_its_history_expired() {
        for claimant in [Some("luiz"), None] {
            let free_at = |now| deleted_name_free("zoe", DELETED, claimant, now, TWO_WEEKS);
            assert!(!free_at(DELETED), "{claimant:?}");
            assert!(!free_at(DELETED + TWO_WEEKS - 1), "{claimant:?}");
            assert!(free_at(DELETED + TWO_WEEKS), "{claimant:?}");
            assert!(free_at(DELETED + 3 * TWO_WEEKS), "{claimant:?}");
        }
    }

    #[test]
    fn the_wait_follows_the_history_expiry() {
        let day = 24 * 60 * 60;
        assert!(deleted_name_free("zoe", DELETED, None, DELETED + day, day));
        assert!(!deleted_name_free(
            "zoe",
            DELETED,
            None,
            DELETED + day,
            TWO_WEEKS
        ));
    }

    #[test]
    fn members_who_havent_confirmed_their_email_make_none() {
        assert_eq!(remaining(false, 0), 0);
        assert_eq!(may_create(false, 0), Err(Refusal::Unverified));
        assert_eq!(may_create(false, MAX_LIVE), Err(Refusal::Unverified));
    }
}
