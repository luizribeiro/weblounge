//! Ergo's chat history, as far as concierge goes by it: how long the
//! server keeps messages, which Ergo's `history.restrictions.expire-time`
//! sets and concierge is handed.

const NANOS_PER_SECOND: u128 = 1_000_000_000;

/// Ergo's duration units, in nanoseconds. A month is 30 days and a year
/// 365, as Ergo counts them.
const UNITS: [(&str, u128); 12] = [
    ("ns", 1),
    ("us", 1_000),
    ("µs", 1_000),
    ("μs", 1_000),
    ("ms", 1_000_000),
    ("s", NANOS_PER_SECOND),
    ("m", 60 * NANOS_PER_SECOND),
    ("h", 60 * 60 * NANOS_PER_SECOND),
    ("d", 24 * 60 * 60 * NANOS_PER_SECOND),
    ("w", 7 * 24 * 60 * 60 * NANOS_PER_SECOND),
    ("mo", 30 * 24 * 60 * 60 * NANOS_PER_SECOND),
    ("y", 365 * 24 * 60 * 60 * NANOS_PER_SECOND),
];

/// Ergo keeps durations in a Go `time.Duration`, which holds no more.
const MAX_NANOS: u128 = i64::MAX as u128;

/// How long Ergo keeps chat history, in whole seconds rounded up, from an
/// expire-time written the way Ergo reads it: numbers with units, summed,
/// such as `2w`, `14d`, `1d12h` or `1.5h`. None for one Ergo refuses; for
/// no time at all, which Ergo takes as keeping history forever; and for a
/// negative one, which Ergo reads but which keeps nothing.
pub fn parse_expire_time(setting: &str) -> Option<i64> {
    let mut rest = setting.strip_prefix('+').unwrap_or(setting);
    if rest.is_empty() {
        return None;
    }
    let mut nanos: u128 = 0;
    while !rest.is_empty() {
        let (term, after) = parse_term(rest)?;
        nanos = nanos
            .checked_add(term)
            .filter(|&nanos| nanos <= MAX_NANOS)?;
        rest = after;
    }
    let seconds = nanos.div_ceil(NANOS_PER_SECOND);
    (seconds > 0).then_some(seconds as i64)
}

/// The nanoseconds a number and its unit at the start of `input` stand
/// for, and what follows them.
fn parse_term(input: &str) -> Option<(u128, &str)> {
    let (whole, rest) = split_digits(input);
    let (fraction, rest) = match rest.strip_prefix('.') {
        Some(rest) => split_digits(rest),
        None => ("", rest),
    };
    if whole.is_empty() && fraction.is_empty() {
        return None;
    }
    let unit_length = rest
        .find(|c: char| c == '.' || c.is_ascii_digit())
        .unwrap_or(rest.len());
    let (unit, rest) = rest.split_at(unit_length);
    let (_, per_unit) = UNITS.iter().find(|(name, _)| *name == unit)?;
    let whole: u128 = if whole.is_empty() {
        0
    } else {
        whole.parse().ok()?
    };
    let nanos = whole
        .checked_mul(*per_unit)?
        .checked_add(fraction_of(fraction, *per_unit))?;
    Some((nanos, rest))
}

fn split_digits(input: &str) -> (&str, &str) {
    let end = input
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(input.len());
    input.split_at(end)
}

/// The nanoseconds `.digits` of a unit `per_unit` long stand for, rounded
/// down. Digits past the nanosecond change nothing, so they're dropped.
fn fraction_of(digits: &str, per_unit: u128) -> u128 {
    let digits = &digits[..digits.len().min(18)];
    if digits.is_empty() {
        return 0;
    }
    let numerator: u128 = digits.parse().unwrap_or(0);
    numerator * per_unit / 10u128.pow(digits.len() as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    const HOUR: i64 = 60 * 60;
    const DAY: i64 = 24 * HOUR;

    #[test]
    fn reads_the_units_retention_is_written_in() {
        assert_eq!(parse_expire_time("2w"), Some(14 * DAY));
        assert_eq!(parse_expire_time("14d"), Some(14 * DAY));
        assert_eq!(parse_expire_time("336h"), Some(14 * DAY));
        assert_eq!(parse_expire_time("1mo"), Some(30 * DAY));
        assert_eq!(parse_expire_time("1y"), Some(365 * DAY));
        assert_eq!(parse_expire_time("90m"), Some(90 * 60));
        assert_eq!(parse_expire_time("3600s"), Some(HOUR));
    }

    #[test]
    fn sums_terms_and_reads_fractions_as_ergo_does() {
        assert_eq!(parse_expire_time("1w2d"), Some(9 * DAY));
        assert_eq!(
            parse_expire_time("1d12h30m"),
            Some(DAY + 12 * HOUR + 30 * 60)
        );
        assert_eq!(parse_expire_time("1.5d"), Some(DAY + 12 * HOUR));
        assert_eq!(parse_expire_time(".5h"), Some(30 * 60));
        assert_eq!(parse_expire_time("2.w"), Some(14 * DAY));
        assert_eq!(parse_expire_time("+2w"), Some(14 * DAY));
    }

    #[test]
    fn rounds_a_part_of_a_second_up() {
        assert_eq!(parse_expire_time("1500ms"), Some(2));
        assert_eq!(parse_expire_time("1ns"), Some(1));
        assert_eq!(parse_expire_time("1µs"), Some(1));
        assert_eq!(parse_expire_time("1h1ns"), Some(HOUR + 1));
    }

    #[test]
    fn refuses_what_ergo_refuses() {
        for setting in [
            "",
            "+",
            "2",
            "w",
            "2 w",
            "2W",
            "2weeks",
            "two weeks",
            "2w3",
            "1.2.3h",
            ".h",
            "2w-1d",
        ] {
            assert_eq!(parse_expire_time(setting), None, "{setting:?}");
        }
    }

    #[test]
    fn refuses_history_kept_forever_or_for_negative_time() {
        for setting in ["0", "0s", "0w0d", "-2w"] {
            assert_eq!(parse_expire_time(setting), None, "{setting:?}");
        }
    }

    #[test]
    fn refuses_more_than_ergo_can_hold() {
        assert_eq!(parse_expire_time("292y"), Some(292 * 365 * DAY));
        assert_eq!(parse_expire_time("293y"), None);
        assert_eq!(parse_expire_time("99999999999999999999999y"), None);
    }
}
