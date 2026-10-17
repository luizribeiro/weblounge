//! Small pieces concierge, the outbox and the mailer share.

pub mod config;
pub mod email;
pub mod random;
pub mod recent;
pub mod sqlite;

use std::time::SystemTime;

/// The current time as the databases store times, in Unix seconds.
pub fn now() -> i64 {
    let since_epoch = SystemTime::UNIX_EPOCH
        .elapsed()
        .expect("the clock is past 1970");
    since_epoch
        .as_secs()
        .try_into()
        .expect("the clock is before 2262")
}

#[cfg(test)]
mod tests {
    #[test]
    fn now_is_in_seconds() {
        let october_2025 = 1_760_000_000;
        assert!((october_2025..october_2025 * 2).contains(&super::now()));
    }
}
