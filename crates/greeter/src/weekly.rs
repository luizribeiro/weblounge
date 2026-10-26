//! The weekly thread in #lounge, which luiz starts: every Monday at 10:00
//! in California, 13:00 on the east coast, the greeter reminds him, with
//! a prompt he may use. A reminder the greeter couldn't send that morning
//! is dropped by evening rather than sent late.

use jiff::{Timestamp, civil::Weekday, tz::TimeZone};

const TIME_ZONE: &str = "America/Los_Angeles";
const HOUR: i8 = 10;
/// How long after 10:00 a reminder is still worth sending.
const WINDOW: i64 = 12 * 60 * 60;

/// Prompts for people with day jobs, who talk about work in general terms
/// and about side projects in detail. One a week, in turn.
const PROMPTS: [&str; 8] = [
    "What's on your plate this week? Work in general terms, side projects in detail.",
    "What are you trying to ship this week, and what's in the way?",
    "What's a decision you're weighing this week, at work or outside it?",
    "What's the most interesting bug, incident or postmortem you've seen lately?",
    "What are you reading this week: a paper, a design doc, a good thread?",
    "What would make this week a win for you?",
    "What's something you changed your mind about recently?",
    "Who's hiring, who's looking, and what are you hearing about the market?",
];

/// The week a reminder is due for at `now`, named by its Monday's date,
/// if it's Monday between 10:00 and the end of the window in California.
pub fn due(now: i64) -> Option<String> {
    let tz = TimeZone::get(TIME_ZONE).expect("the bundled time zone database has it");
    let today = Timestamp::from_second(now).ok()?.to_zoned(tz.clone());
    if today.weekday() != Weekday::Monday {
        return None;
    }
    let ten = today
        .date()
        .at(HOUR, 0, 0, 0)
        .to_zoned(tz)
        .ok()?
        .timestamp();
    let since_ten = now - ten.as_second();
    (0..WINDOW)
        .contains(&since_ten)
        .then(|| today.date().to_string())
}

/// The reminder for the week named `week`, with its prompt.
pub fn reminder(week: &str) -> String {
    let prompt = PROMPTS[week_number(week) % PROMPTS.len()];
    format!("It's Monday: time for this week's thread in #lounge. A prompt you could use: {prompt}")
}

/// The ISO week of the Monday named `week`, which [`due`] gave.
fn week_number(week: &str) -> usize {
    let date: jiff::civil::Date = week.parse().expect("due names weeks by their date");
    usize::try_from(date.iso_week_date().week()).expect("weeks are numbered from 1")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(instant: &str) -> i64 {
        instant.parse::<Timestamp>().unwrap().as_second()
    }

    #[test]
    fn is_due_monday_from_ten_in_california() {
        assert_eq!(due(at("2026-10-26T16:59:59Z")), None);
        assert_eq!(
            due(at("2026-10-26T17:00:00Z")).as_deref(),
            Some("2026-10-26")
        );
        assert_eq!(
            due(at("2026-10-27T04:59:59Z")).as_deref(),
            Some("2026-10-26")
        );
        assert_eq!(due(at("2026-10-27T05:00:00Z")), None);
    }

    #[test]
    fn follows_california_across_daylight_saving_time() {
        assert_eq!(due(at("2026-11-02T17:00:00Z")), None);
        assert_eq!(
            due(at("2026-11-02T18:00:00Z")).as_deref(),
            Some("2026-11-02")
        );
        assert_eq!(due(at("2026-03-09T16:59:59Z")), None);
        assert_eq!(
            due(at("2026-03-09T17:00:00Z")).as_deref(),
            Some("2026-03-09")
        );
    }

    #[test]
    fn is_never_due_on_other_days() {
        for day in 27..=31 {
            assert_eq!(due(at(&format!("2026-10-{day}T18:00:00Z"))), None, "{day}");
        }
    }

    #[test]
    fn takes_the_prompts_in_turn_one_a_week() {
        let first = reminder("2026-10-26");
        assert!(
            first.starts_with("It's Monday: time for this week's thread in #lounge."),
            "{first}"
        );
        assert_ne!(first, reminder("2026-11-02"));
        assert_eq!(first, reminder("2026-12-21"));
    }
}
