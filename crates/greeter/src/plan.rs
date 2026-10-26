//! What the greeter does next for a newcomer, from what it did so far and
//! whether they have a session attached now. It decides; the greeter
//! acts and records each step once it's done, so every tick starts from
//! what really happened.

use concierge::{db::Arrival, welcome_rules};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    /// Note that the newcomer was first seen with a session attached.
    Arrive,
    /// Welcome them in #lounge.
    Welcome,
    /// Tell their inviter, by direct message, that they were welcomed.
    TellInviter,
}

/// Whether the greeter needs to know if the newcomer is connected now.
pub fn needs_presence(arrival: &Arrival) -> bool {
    arrival.arrived_at.is_none()
}

/// The steps due for `arrival` at `now`, in the order to take them, given
/// whether the newcomer has a session `attached`.
pub fn due(arrival: &Arrival, attached: bool, now: i64) -> Vec<Step> {
    let mut steps = Vec::new();
    let first_sight = attached && arrival.arrived_at.is_none();
    if first_sight {
        steps.push(Step::Arrive);
    }
    let seen = first_sight || arrival.arrived_at.is_some();
    let welcoming =
        arrival.announced_at.is_none() && welcome_rules::welcome_due(arrival.joined_at, seen, now);
    if welcoming {
        steps.push(Step::Welcome);
    }
    let announced_at = arrival.announced_at.or(welcoming.then_some(now));
    if let Some(announced_at) = announced_at
        && arrival.inviter.is_some()
        && arrival.inviter_told_at.is_none()
        && !welcome_rules::stale(arrival.joined_at, now)
        && welcome_rules::still_trying(announced_at, now)
    {
        steps.push(Step::TellInviter);
    }
    steps
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use concierge::name::MemberName;
    use welcome_rules::{FALLBACK, STALE_AFTER};

    pub const JOINED: i64 = 1_760_000_000;

    /// erin, whom luiz invited at JOINED, before the greeter did anything.
    pub fn erin() -> Arrival {
        Arrival {
            account: MemberName::new("erin"),
            inviter: Some(MemberName::new("luiz")),
            joined_at: JOINED,
            arrived_at: None,
            announced_at: None,
            inviter_told_at: None,
            nudged_at: None,
            oriented_at: None,
        }
    }

    /// frank, who came in through a root invite at JOINED.
    fn frank() -> Arrival {
        Arrival {
            account: MemberName::new("frank"),
            inviter: None,
            ..erin()
        }
    }

    /// `arrival`, once seen and welcomed at `at`.
    fn welcomed(arrival: Arrival, at: i64) -> Arrival {
        Arrival {
            arrived_at: Some(at),
            announced_at: Some(at),
            ..arrival
        }
    }

    /// erin, once seen, welcomed and announced to luiz at `at`.
    fn told(at: i64) -> Arrival {
        Arrival {
            inviter_told_at: Some(at),
            ..welcomed(erin(), at)
        }
    }

    #[test]
    fn notes_the_first_sight_of_a_session_once() {
        assert_eq!(due(&frank(), true, JOINED), [Step::Arrive, Step::Welcome]);
        assert_eq!(due(&told(JOINED + 5), true, JOINED + 10), []);
    }

    #[test]
    fn asks_about_presence_only_until_the_first_sight() {
        assert!(needs_presence(&erin()));
        assert!(!needs_presence(&told(JOINED + 5)));
    }

    #[test]
    fn welcomes_at_first_sight_or_once_the_fallback_comes() {
        assert_eq!(due(&frank(), false, JOINED + FALLBACK - 1), []);
        assert_eq!(due(&frank(), false, JOINED + FALLBACK), [Step::Welcome]);
        let seen_earlier = Arrival {
            arrived_at: Some(JOINED + 1),
            ..frank()
        };
        assert_eq!(due(&seen_earlier, false, JOINED + 2), [Step::Welcome]);
    }

    #[test]
    fn never_welcomes_twice_or_late() {
        let announced = welcomed(frank(), JOINED + FALLBACK);
        assert_eq!(due(&announced, false, JOINED + FALLBACK + 5), []);
        assert_eq!(due(&erin(), true, JOINED + STALE_AFTER), [Step::Arrive]);
    }

    #[test]
    fn tells_the_inviter_right_after_the_welcome() {
        assert_eq!(
            due(&erin(), true, JOINED),
            [Step::Arrive, Step::Welcome, Step::TellInviter]
        );
        let announced = welcomed(erin(), JOINED);
        assert_eq!(due(&announced, false, JOINED + 5), [Step::TellInviter]);
        assert_eq!(due(&told(JOINED), false, JOINED + 5), []);
    }

    #[test]
    fn tells_nobody_of_a_root_invitee_or_a_removed_inviters() {
        assert_eq!(due(&welcomed(frank(), JOINED), false, JOINED + 5), []);
    }

    #[test]
    fn gives_up_on_an_inviter_who_cant_be_reached_for_a_day() {
        let announced = welcomed(erin(), JOINED);
        let day = welcome_rules::RETRY_FOR;
        assert_eq!(
            due(&announced, false, JOINED + day - 1),
            [Step::TellInviter]
        );
        assert_eq!(due(&announced, false, JOINED + day), []);
    }

    #[test]
    fn never_tells_the_inviter_once_the_arrival_is_stale() {
        let late = Arrival {
            announced_at: Some(JOINED + STALE_AFTER - 1),
            ..erin()
        };
        assert_eq!(due(&late, false, JOINED + STALE_AFTER), []);
    }
}
