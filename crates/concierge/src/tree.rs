//! The invite tree: who invited whom, from the members' rows alone.

use std::collections::HashMap;

use crate::name::MemberName;

/// A member's place in the tree, as the database keeps it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Branch {
    pub account: MemberName,
    pub invited_by: Option<MemberName>,
    pub joined_at: i64,
    pub removed: bool,
}

/// The tree in reading order, as the nested list a page renders: each
/// member, with the members they invited inside a list of their own.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Line {
    Member {
        account: String,
        joined_on: String,
        removed: bool,
        has_invitees: bool,
    },
    /// Starts the list of the previous member's invitees.
    Open,
    /// Ends that list.
    Close,
}

/// Lays `branches` out from the roots, the members nobody invited, down;
/// everyone in the order they joined.
pub fn lines(mut branches: Vec<Branch>) -> Vec<Line> {
    branches
        .sort_by(|a, b| (a.joined_at, a.account.as_str()).cmp(&(b.joined_at, b.account.as_str())));
    let mut invitees: HashMap<Option<MemberName>, Vec<Branch>> = HashMap::new();
    for branch in branches {
        invitees
            .entry(branch.invited_by.clone())
            .or_default()
            .push(branch);
    }
    let mut lines = Vec::new();
    lay_out(&mut invitees, None, &mut lines);
    lines
}

/// Members whose inviter isn't on the way down from a root can't be
/// reached, and are left out rather than looped over.
fn lay_out(
    invitees: &mut HashMap<Option<MemberName>, Vec<Branch>>,
    inviter: Option<MemberName>,
    lines: &mut Vec<Line>,
) {
    for branch in invitees.remove(&inviter).unwrap_or_default() {
        let key = Some(branch.account.clone());
        let has_invitees = invitees.contains_key(&key);
        lines.push(Line::Member {
            joined_on: date(branch.joined_at),
            account: branch.account.to_string(),
            removed: branch.removed,
            has_invitees,
        });
        if has_invitees {
            lines.push(Line::Open);
            lay_out(invitees, key, lines);
            lines.push(Line::Close);
        }
    }
}

/// `unix` seconds as a UTC date, such as 2026-10-14.
pub fn date(unix: i64) -> String {
    let (year, month, day) = civil(unix);
    format!("{year:04}-{month:02}-{day:02}")
}

const MONTHS: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];

/// A day as members read it, such as "24 October", in UTC: for days
/// within weeks of today, where the year goes without saying.
pub fn day_in_words(unix: i64) -> String {
    let (_, month, day) = civil(unix);
    let month = usize::try_from(month - 1).expect("months run from 1");
    format!("{day} {}", MONTHS[month])
}

/// The UTC year, month (1 to 12) and day of `unix` seconds.
pub fn civil(unix: i64) -> (i64, i64, i64) {
    // Howard Hinnant's days-to-civil algorithm.
    let days = unix.div_euclid(86_400) + 719_468;
    let era = days.div_euclid(146_097);
    let day_of_era = days - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_index = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_index + 2) / 5 + 1;
    let month = if month_index < 10 {
        month_index + 3
    } else {
        month_index - 9
    };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn branch(account: &str, invited_by: Option<&str>, joined_at: i64) -> Branch {
        Branch {
            account: MemberName::new(account),
            invited_by: invited_by.map(MemberName::new),
            joined_at,
            removed: false,
        }
    }

    fn outline(lines: &[Line]) -> String {
        lines
            .iter()
            .map(|line| match line {
                Line::Member { account, .. } => account.as_str(),
                Line::Open => "(",
                Line::Close => ")",
            })
            .collect::<Vec<_>>()
            .join(" ")
    }

    #[test]
    fn nests_invitees_under_their_inviter_in_the_order_they_joined() {
        let lines = lines(vec![
            branch("carol", Some("alice"), 30),
            branch("dave", Some("bob"), 40),
            branch("bob", Some("alice"), 20),
            branch("alice", None, 10),
            branch("erin", None, 5),
        ]);
        assert_eq!(outline(&lines), "erin alice ( bob ( dave ) carol )");
    }

    #[test]
    fn says_who_has_invitees_and_who_left() {
        let mut gone = branch("bob", Some("alice"), 20);
        gone.removed = true;
        let lines = lines(vec![branch("alice", None, 10), gone]);
        assert_eq!(
            lines[0],
            Line::Member {
                account: "alice".into(),
                joined_on: "1970-01-01".into(),
                removed: false,
                has_invitees: true,
            }
        );
        assert!(matches!(
            &lines[2],
            Line::Member {
                removed: true,
                has_invitees: false,
                ..
            }
        ));
    }

    #[test]
    fn matches_inviters_whatever_their_case() {
        let lines = lines(vec![
            branch("Alice", None, 10),
            branch("bob", Some("alice"), 20),
        ]);
        assert_eq!(outline(&lines), "Alice ( bob )");
    }

    #[test]
    fn leaves_out_members_no_root_leads_to() {
        let lines = lines(vec![
            branch("alice", None, 10),
            branch("x", Some("y"), 20),
            branch("y", Some("x"), 30),
        ]);
        assert_eq!(outline(&lines), "alice");
    }

    #[test]
    fn an_empty_tree_has_no_lines() {
        assert!(lines(Vec::new()).is_empty());
    }

    #[test]
    fn dates_are_utc_calendar_days() {
        assert_eq!(date(0), "1970-01-01");
        assert_eq!(date(1_760_000_000), "2025-10-09");
        assert_eq!(date(951_782_400), "2000-02-29");
        assert_eq!(date(1_798_761_599), "2026-12-31");
    }
}
