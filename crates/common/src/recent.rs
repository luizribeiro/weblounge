//! Counting what happened lately, in memory: concierge's rate limits and
//! the mailer's hourly quota.

use std::{
    collections::{HashMap, VecDeque},
    hash::Hash,
    sync::Mutex,
    time::{Duration, Instant},
};

/// Past this many tracked keys, keys not seen in the window are dropped.
const PRUNE_ABOVE: usize = 10_000;

/// When each key was last seen, over a sliding window, for keys seen
/// recently.
pub struct Recent<K> {
    window: Duration,
    times: Mutex<HashMap<K, VecDeque<Instant>>>,
}

impl<K: Eq + Hash> Recent<K> {
    pub fn new(window: Duration) -> Self {
        Self {
            window,
            times: Mutex::default(),
        }
    }

    /// Whether `key` was seen fewer than `limit` times in the window.
    pub fn under(&self, key: &K, limit: usize, now: Instant) -> bool {
        let mut times = self.lock();
        times
            .get_mut(key)
            .is_none_or(|times| self.prune(times, now).len() < limit)
    }

    /// Records `key` if it was seen fewer than `limit` times in the
    /// window; answers whether it did.
    pub fn take(&self, key: K, limit: usize, now: Instant) -> bool {
        let allowed = self.under(&key, limit, now);
        if allowed {
            self.record(key, now);
        }
        allowed
    }

    pub fn record(&self, key: K, now: Instant) {
        let mut times = self.lock();
        if times.len() > PRUNE_ABOVE {
            times.retain(|_, times| !self.prune(times, now).is_empty());
        }
        times.entry(key).or_default().push_back(now);
    }

    pub fn forget(&self, key: &K) {
        self.lock().remove(key);
    }

    /// Forgets that `key` was seen `at`, once.
    pub fn forget_one(&self, key: &K, at: Instant) {
        let mut times = self.lock();
        if let Some(seen) = times.get_mut(key) {
            if let Some(index) = seen.iter().position(|&time| time == at) {
                seen.remove(index);
            }
            if seen.is_empty() {
                times.remove(key);
            }
        }
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.lock().len()
    }

    /// Drops the times that left the window and returns the rest.
    fn prune<'a>(&self, times: &'a mut VecDeque<Instant>, now: Instant) -> &'a VecDeque<Instant> {
        while times
            .front()
            .is_some_and(|&time| now.duration_since(time) >= self.window)
        {
            times.pop_front();
        }
        times
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<K, VecDeque<Instant>>> {
        self.times
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

/// Uses of something shared in the last `window`, up to a limit.
pub struct Quota {
    uses: Recent<()>,
    limit: usize,
}

impl Quota {
    pub fn new(limit: usize, window: Duration) -> Self {
        Self {
            uses: Recent::new(window),
            limit,
        }
    }

    /// Whether another use may go ahead, counting it if so.
    pub fn take(&self, now: Instant) -> bool {
        self.uses.take((), self.limit, now)
    }

    /// Whether another use may go ahead, counting nothing.
    pub fn has_room(&self, now: Instant) -> bool {
        self.uses.under(&(), self.limit, now)
    }

    pub fn record(&self, now: Instant) {
        self.uses.record((), now);
    }

    /// Gives back the use recorded `at`.
    pub fn forget_one(&self, at: Instant) {
        self.uses.forget_one(&(), at);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MINUTE: Duration = Duration::from_secs(60);

    #[test]
    fn a_quota_refills_as_uses_leave_the_window() {
        let (start, hour) = (Instant::now(), Duration::from_secs(3600));
        let quota = Quota::new(3, hour);
        let minute = |minutes: u32| start + MINUTE * minutes;
        for minutes in [0, 10, 20] {
            assert!(quota.take(minute(minutes)));
        }
        assert!(!quota.has_room(minute(59)));
        assert!(!quota.take(minute(59)));
        assert!(quota.has_room(minute(60)));
        assert!(quota.take(minute(60)));
        assert!(!quota.take(minute(69)));
        assert!(quota.take(minute(70)));
    }

    #[test]
    fn forgetting_one_sighting_leaves_the_others() {
        let (recent, start) = (Recent::new(MINUTE), Instant::now());
        let later = start + Duration::from_secs(1);
        for at in [start, later, later] {
            recent.record("alice", at);
        }

        recent.forget_one(&"alice", later);
        assert!(!recent.under(&"alice", 2, later));
        recent.forget_one(&"alice", later);
        assert!(recent.under(&"alice", 2, later));
        recent.forget_one(&"alice", start);
        recent.forget_one(&"bob", start);

        assert_eq!(recent.len(), 0);
    }

    #[test]
    fn forgets_keys_not_seen_in_the_window_once_it_tracks_many() {
        let (recent, start) = (Recent::new(MINUTE), Instant::now());
        for n in 0..=PRUNE_ABOVE {
            recent.record(format!("guess{n}"), start);
        }
        for key in ["alice", "192.0.2.7"] {
            recent.record(key.to_owned(), start + MINUTE);
        }
        assert_eq!(recent.len(), 2);
    }
}
