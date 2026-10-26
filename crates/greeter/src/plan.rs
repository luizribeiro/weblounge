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
    if arrival.announced_at.is_none() && welcome_rules::welcome_due(arrival.joined_at, seen, now) {
        steps.push(Step::Welcome);
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

    /// erin, once she was seen and welcomed at `at`.
    fn welcomed(at: i64) -> Arrival {
        Arrival {
            arrived_at: Some(at),
            announced_at: Some(at),
            ..erin()
        }
    }

    #[test]
    fn notes_the_first_sight_of_a_session_once() {
        assert_eq!(due(&erin(), true, JOINED), [Step::Arrive, Step::Welcome]);
        assert_eq!(due(&welcomed(JOINED + 5), true, JOINED + 10), []);
    }

    #[test]
    fn asks_about_presence_only_until_the_first_sight() {
        assert!(needs_presence(&erin()));
        assert!(!needs_presence(&welcomed(JOINED + 5)));
    }

    #[test]
    fn welcomes_at_first_sight_or_once_the_fallback_comes() {
        assert_eq!(due(&erin(), false, JOINED + FALLBACK - 1), []);
        assert_eq!(due(&erin(), false, JOINED + FALLBACK), [Step::Welcome]);
        let seen_earlier = Arrival {
            arrived_at: Some(JOINED + 1),
            ..erin()
        };
        assert_eq!(due(&seen_earlier, false, JOINED + 2), [Step::Welcome]);
    }

    #[test]
    fn never_welcomes_twice_or_late() {
        let announced = Arrival {
            announced_at: Some(JOINED + FALLBACK),
            ..erin()
        };
        assert_eq!(due(&announced, false, JOINED + FALLBACK + 5), []);
        assert_eq!(due(&erin(), true, JOINED + STALE_AFTER), [Step::Arrive]);
    }
}
