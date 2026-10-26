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
    /// Say hello to them by direct message.
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
    let Some(arrived_at) = arrival.arrived_at.or(first_sight.then_some(now)) else {
        return nudge(arrival, now);
    };
    if arrival.announced_at.is_none() {
        steps.push(Step::Welcome);
    }
    let announced_at = arrival.announced_at.unwrap_or(now);
    if arrival.inviter_told_at.is_none() && welcome_rules::still_trying(announced_at, now) {
        steps.push(Step::TellInviter);
    }
    if arrival.oriented_at.is_none() && welcome_rules::still_trying(arrived_at, now) {
        steps.push(Step::Orient);
    }
    steps
}

/// The nudge, if it's due for a newcomer never seen.
fn nudge(arrival: &Arrival, now: i64) -> Vec<Step> {
    let nudge_at = welcome_rules::nudge_at(arrival.joined_at);
    let due = arrival.nudged_at.is_none()
        && now >= nudge_at
        && welcome_rules::still_trying(nudge_at, now);
    if due { vec![Step::Nudge] } else { vec![] }
}

#[cfg(test)]
mod tests {
    use super::*;
    use concierge::name::MemberName;
    use welcome_rules::{NUDGE_AFTER, RETRY_FOR};

    const JOINED: i64 = 1_760_000_000;
    const ALL: [Step; 4] = [Step::Arrive, Step::Welcome, Step::TellInviter, Step::Orient];

    /// erin, whom luiz invited at JOINED, before the greeter did anything.
    fn erin() -> Arrival {
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

    /// erin, seen and welcomed at `at`, and nothing more.
    fn welcomed(at: i64) -> Arrival {
        Arrival {
            arrived_at: Some(at),
            announced_at: Some(at),
            ..erin()
        }
    }

    /// erin, once everything was done at `at`.
    fn done(at: i64) -> Arrival {
        Arrival {
            inviter_told_at: Some(at),
            oriented_at: Some(at),
            ..welcomed(at)
        }
    }

    #[test]
    fn does_everything_at_first_sight_in_order() {
        assert_eq!(due(&erin(), true, JOINED + 5), ALL);
    }

    #[test]
    fn welcomes_nobody_while_they_are_away() {
        assert_eq!(due(&erin(), false, JOINED + 60 * 60), []);
    }

    #[test]
    fn welcomes_a_first_sight_however_late_it_comes() {
        let late = JOINED + 13 * 24 * 60 * 60;
        let nudged = Arrival {
            nudged_at: Some(JOINED + NUDGE_AFTER),
            ..erin()
        };
        assert_eq!(due(&nudged, true, late), ALL);
    }

    #[test]
    fn does_nothing_twice() {
        assert_eq!(due(&done(JOINED + 5), true, JOINED + 10), []);
        assert!(!needs_presence(&done(JOINED + 5)));
    }

    #[test]
    fn a_welcome_that_failed_is_tried_on_later_ticks() {
        let seen = Arrival {
            arrived_at: Some(JOINED),
            ..erin()
        };
        assert_eq!(due(&seen, false, JOINED + 5), ALL[1..]);
        assert_eq!(
            due(&seen, false, JOINED + 3 * 24 * 60 * 60),
            [Step::Welcome, Step::TellInviter]
        );
    }

    #[test]
    fn tells_the_inviter_after_the_welcome_and_gives_up_after_a_day() {
        let welcomed = Arrival {
            oriented_at: Some(JOINED),
            ..welcomed(JOINED)
        };
        assert_eq!(
            due(&welcomed, false, JOINED + RETRY_FOR - 1),
            [Step::TellInviter]
        );
        assert_eq!(due(&welcomed, false, JOINED + RETRY_FOR), []);
    }

    #[test]
    fn says_hello_after_the_welcome_and_gives_up_after_a_day() {
        let told = Arrival {
            inviter_told_at: Some(JOINED),
            ..welcomed(JOINED)
        };
        assert_eq!(due(&told, false, JOINED + RETRY_FOR - 1), [Step::Orient]);
        assert_eq!(due(&told, false, JOINED + RETRY_FOR), []);
    }

    #[test]
    fn nudges_the_inviter_once_two_days_in_if_the_newcomer_never_came() {
        let nudge_at = welcome_rules::nudge_at(JOINED);
        assert_eq!(due(&erin(), false, nudge_at - 1), []);
        assert_eq!(due(&erin(), false, nudge_at), [Step::Nudge]);
        assert_eq!(due(&erin(), false, nudge_at + RETRY_FOR - 1), [Step::Nudge]);
        assert_eq!(due(&erin(), false, nudge_at + RETRY_FOR), []);
        let nudged = Arrival {
            nudged_at: Some(nudge_at),
            ..erin()
        };
        assert_eq!(due(&nudged, false, nudge_at + 5), []);
    }

    #[test]
    fn nudges_nobody_about_a_newcomer_who_came() {
        let nudge_at = welcome_rules::nudge_at(JOINED);
        assert_eq!(due(&done(JOINED + 1), false, nudge_at), []);
        assert_eq!(due(&erin(), true, nudge_at), ALL);
    }
}
