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

/// Keeps this process's memory out of core dumps, and out of reach of
/// other processes running as its user.
pub fn make_undumpable() -> std::io::Result<()> {
    let off: libc::c_ulong = 0;
    // SAFETY: PR_SET_DUMPABLE reads its one argument and no memory of ours.
    match unsafe { libc::prctl(libc::PR_SET_DUMPABLE, off) } {
        0 => Ok(()),
        _ => Err(std::io::Error::last_os_error()),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn now_is_in_seconds() {
        let october_2025 = 1_760_000_000;
        assert!((october_2025..october_2025 * 2).contains(&super::now()));
    }

    #[test]
    fn an_undumpable_process_says_so() {
        super::make_undumpable().unwrap();
        // SAFETY: PR_GET_DUMPABLE takes no argument and touches no memory.
        assert_eq!(unsafe { libc::prctl(libc::PR_GET_DUMPABLE) }, 0);
    }
}
