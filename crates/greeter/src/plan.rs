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
    /// Say hello to them by direct message, once they've been seen.
    Orient,
    /// Tell their inviter that they haven't connected yet.
    Nudge,
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
    // A step due this tick counts as done at `now` for the ones after it.
    let arrived_at = arrival.arrived_at.or(first_sight.then_some(now));
    let welcoming = arrival.announced_at.is_none()
        && welcome_rules::welcome_due(arrival.joined_at, arrived_at.is_some(), now);
    if welcoming {
        steps.push(Step::Welcome);
    }
    let announced_at = arrival.announced_at.or(welcoming.then_some(now));
    if let Some(announced_at) = announced_at
        && arrival.inviter_told_at.is_none()
        && !welcome_rules::stale(arrival.joined_at, now)
        && welcome_rules::still_trying(announced_at, now)
    {
        steps.push(Step::TellInviter);
    }
    if let Some(arrived_at) = arrived_at
        && arrival.oriented_at.is_none()
        && welcome_rules::still_trying(arrived_at, now)
    {
        steps.push(Step::Orient);
    }
    let nudge_at = welcome_rules::nudge_at(arrival.joined_at);
    if arrived_at.is_none()
        && arrival.nudged_at.is_none()
        && now >= nudge_at
        && welcome_rules::still_trying(nudge_at, now)
    {
        steps.push(Step::Nudge);
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
            inviter: MemberName::new("luiz"),
            joined_at: JOINED,
            arrived_at: None,
            announced_at: None,
            inviter_told_at: None,
            nudged_at: None,
            oriented_at: None,
        }
    }

    /// erin, once seen, welcomed and said hello to at `at`.
    fn welcomed(at: i64) -> Arrival {
        Arrival {
            arrived_at: Some(at),
            announced_at: Some(at),
            oriented_at: Some(at),
            ..erin()
        }
    }

    /// erin, once everything was done at `at`.
    fn told(at: i64) -> Arrival {
        Arrival {
            inviter_told_at: Some(at),
            ..welcomed(at)
        }
    }

    #[test]
    fn notes_the_first_sight_of_a_session_once() {
        assert_eq!(
            due(&erin(), true, JOINED),
            [Step::Arrive, Step::Welcome, Step::TellInviter, Step::Orient]
        );
        assert_eq!(due(&told(JOINED + 5), true, JOINED + 10), []);
    }

    #[test]
    fn asks_about_presence_only_until_the_first_sight() {
        assert!(needs_presence(&erin()));
        assert!(!needs_presence(&told(JOINED + 5)));
    }

    #[test]
    fn welcomes_at_first_sight_or_once_the_fallback_comes() {
        assert_eq!(due(&erin(), false, JOINED + FALLBACK - 1), []);
        assert_eq!(
            due(&erin(), false, JOINED + FALLBACK),
            [Step::Welcome, Step::TellInviter]
        );
    }

    #[test]
    fn never_welcomes_twice_or_late() {
        assert_eq!(
            due(&told(JOINED + FALLBACK), false, JOINED + FALLBACK + 5),
            []
        );
        assert_eq!(
            due(&erin(), true, JOINED + STALE_AFTER),
            [Step::Arrive, Step::Orient]
        );
    }

    #[test]
    fn says_hello_once_the_newcomer_is_seen_and_only_then() {
        let unseen = Arrival {
            announced_at: Some(JOINED + FALLBACK),
            inviter_told_at: Some(JOINED + FALLBACK),
            ..erin()
        };
        assert_eq!(due(&unseen, false, JOINED + FALLBACK + 5), []);
        assert_eq!(
            due(&unseen, true, JOINED + 3 * 24 * 60 * 60),
            [Step::Arrive, Step::Orient]
        );
        assert_eq!(due(&told(JOINED), false, JOINED + 5), []);
    }

    #[test]
    fn gives_up_on_a_hello_that_cant_get_through_for_a_day() {
        let seen = Arrival {
            oriented_at: None,
            ..told(JOINED)
        };
        let day = welcome_rules::RETRY_FOR;
        assert_eq!(due(&seen, false, JOINED + day - 1), [Step::Orient]);
        assert_eq!(due(&seen, false, JOINED + day), []);
    }

    #[test]
    fn tells_the_inviter_right_after_the_welcome() {
        assert_eq!(
            due(&welcomed(JOINED), false, JOINED + 5),
            [Step::TellInviter]
        );
        assert_eq!(due(&told(JOINED), false, JOINED + 5), []);
    }

    #[test]
    fn gives_up_on_an_inviter_who_cant_be_reached_for_a_day() {
        let announced = welcomed(JOINED);
        let day = welcome_rules::RETRY_FOR;
        assert_eq!(
            due(&announced, false, JOINED + day - 1),
            [Step::TellInviter]
        );
        assert_eq!(due(&announced, false, JOINED + day), []);
    }

    /// erin, welcomed by the fallback and announced to luiz, unseen since.
    fn unseen() -> Arrival {
        Arrival {
            announced_at: Some(JOINED + FALLBACK),
            inviter_told_at: Some(JOINED + FALLBACK),
            ..erin()
        }
    }

    #[test]
    fn nudges_the_inviter_once_two_days_in_if_the_newcomer_never_came() {
        let nudge_at = welcome_rules::nudge_at(JOINED);
        assert_eq!(due(&unseen(), false, nudge_at - 1), []);
        assert_eq!(due(&unseen(), false, nudge_at), [Step::Nudge]);
        let nudged = Arrival {
            nudged_at: Some(nudge_at),
            ..unseen()
        };
        assert_eq!(due(&nudged, false, nudge_at + 5), []);
    }

    #[test]
    fn nudges_nobody_about_a_newcomer_who_came() {
        let nudge_at = welcome_rules::nudge_at(JOINED);
        assert_eq!(due(&unseen(), true, nudge_at), [Step::Arrive, Step::Orient]);
        let came = Arrival {
            arrived_at: Some(JOINED + 1),
            oriented_at: Some(JOINED + 1),
            ..unseen()
        };
        assert_eq!(due(&came, false, nudge_at), []);
    }

    #[test]
    fn gives_up_on_a_nudge_that_cant_get_through_for_a_day() {
        let nudge_at = welcome_rules::nudge_at(JOINED);
        let day = welcome_rules::RETRY_FOR;
        assert_eq!(due(&unseen(), false, nudge_at + day - 1), [Step::Nudge]);
        assert_eq!(due(&unseen(), false, nudge_at + day), []);
    }

    #[test]
    fn never_tells_the_inviter_once_the_arrival_is_stale() {
        let late = Arrival {
            announced_at: Some(JOINED + STALE_AFTER - 1),
            nudged_at: Some(JOINED + STALE_AFTER - 1),
            ..erin()
        };
        assert_eq!(due(&late, false, JOINED + STALE_AFTER), []);
    }
}
