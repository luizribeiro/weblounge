//! What the greeter does next for a newcomer, from what it did so far and
//! whether they have a session attached now. It decides; the greeter
//! acts and records each step once it's done, so every tick starts from
//! what really happened.

use concierge::db::Arrival;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    /// Note that the newcomer was first seen with a session attached.
    Arrive,
}

/// Whether the greeter needs to know if the newcomer is connected now.
pub fn needs_presence(arrival: &Arrival) -> bool {
    arrival.arrived_at.is_none()
}

/// The steps due for `arrival` at `now`, in the order to take them, given
/// whether the newcomer has a session `attached`.
pub fn due(arrival: &Arrival, attached: bool, _now: i64) -> Vec<Step> {
    let mut steps = Vec::new();
    if attached && arrival.arrived_at.is_none() {
        steps.push(Step::Arrive);
    }
    steps
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use concierge::name::MemberName;

    pub const JOINED: i64 = 1_760_000_000;

    /// erin, whom luiz invited at JOINED, before the greeter did anything.
    pub fn erin() -> Arrival {
        Arrival {
            account: MemberName::new("erin"),
            inviter: Some(MemberName::new("luiz")),
            joined_at: JOINED,
            arrived_at: None,
        }
    }

    #[test]
    fn notes_the_first_sight_of_a_session_once() {
        assert_eq!(due(&erin(), false, JOINED), []);
        assert_eq!(due(&erin(), true, JOINED), [Step::Arrive]);
        let arrived = Arrival {
            arrived_at: Some(JOINED + 5),
            ..erin()
        };
        assert_eq!(due(&arrived, true, JOINED + 10), []);
    }

    #[test]
    fn asks_about_presence_only_until_the_first_sight() {
        assert!(needs_presence(&erin()));
        let arrived = Arrival {
            arrived_at: Some(JOINED + 5),
            ..erin()
        };
        assert!(!needs_presence(&arrived));
    }
}
