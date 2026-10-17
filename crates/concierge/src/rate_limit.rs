//! Slows down password guessing on the website. Ergo doesn't throttle
//! `check_auth`, so this is the only guard; it lives in memory, and failed
//! website attempts never lock anyone out of IRC.

use std::{
    collections::{HashMap, VecDeque},
    fmt,
    hash::Hash,
    net::{IpAddr, SocketAddr},
    sync::Mutex,
    time::{Duration, Instant},
};

use axum::{
    extract::{ConnectInfo, FromRequestParts},
    http::{HeaderMap, request::Parts},
};

/// How long a failed attempt counts against its account and address.
pub const WINDOW: Duration = Duration::from_secs(15 * 60);
/// Enough for a member fumbling their own password a few times.
pub const PER_ACCOUNT: usize = 5;
/// Enough for a household or an office behind one address.
pub const PER_ADDRESS: usize = 20;
/// Past this many tracked keys, keys without recent failures are dropped.
const PRUNE_ABOVE: usize = 10_000;

/// The header nginx puts the client's address in, replacing whatever the
/// client sent. Concierge listens on loopback only, so nothing but nginx
/// can set it.
const REAL_IP: &str = "x-real-ip";

/// Where a request comes from: what nginx says, or the peer itself when
/// there's no nginx in front, as in the dev loop. Kept in memory only, and
/// never logged.
pub struct ClientAddress(pub Option<IpAddr>);

impl ClientAddress {
    fn of(headers: &HeaderMap, peer: Option<SocketAddr>) -> Self {
        match headers.get(REAL_IP) {
            Some(value) => Self(value.to_str().ok().and_then(|value| value.parse().ok())),
            None => Self(peer.map(|peer| peer.ip())),
        }
    }
}

impl<S: Send + Sync> FromRequestParts<S> for ClientAddress {
    type Rejection = std::convert::Infallible;

    async fn from_request_parts(parts: &mut Parts, _: &S) -> Result<Self, Self::Rejection> {
        let peer = parts.extensions.get::<ConnectInfo<SocketAddr>>();
        Ok(Self::of(
            &parts.headers,
            peer.map(|ConnectInfo(peer)| *peer),
        ))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum Key {
    /// Lowercased, as Ergo's casemapping folds names.
    Account(String),
    Address(IpAddr),
}

/// When each key was last seen, over a sliding window, for keys seen
/// recently.
struct Recent<K> {
    window: Duration,
    times: Mutex<HashMap<K, VecDeque<Instant>>>,
}

impl<K: Eq + Hash> Recent<K> {
    fn new(window: Duration) -> Self {
        Self {
            window,
            times: Mutex::default(),
        }
    }

    /// Whether `key` was seen fewer than `limit` times in the window.
    fn under(&self, key: &K, limit: usize, now: Instant) -> bool {
        let mut times = self.lock();
        times
            .get_mut(key)
            .is_none_or(|times| self.prune(times, now).len() < limit)
    }

    /// Records `key` if it was seen fewer than `limit` times in the
    /// window; answers whether it did.
    fn take(&self, key: K, limit: usize, now: Instant) -> bool {
        let allowed = self.under(&key, limit, now);
        if allowed {
            self.record(key, now);
        }
        allowed
    }

    fn record(&self, key: K, now: Instant) {
        let mut times = self.lock();
        if times.len() > PRUNE_ABOVE {
            times.retain(|_, times| !self.prune(times, now).is_empty());
        }
        times.entry(key).or_default().push_back(now);
    }

    fn forget(&self, key: &K) {
        self.lock().remove(key);
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

/// Failed sign-in attempts in the last [`WINDOW`], by account and by client
/// address.
pub struct Limiter {
    failures: Recent<Key>,
}

impl Default for Limiter {
    fn default() -> Self {
        Self {
            failures: Recent::new(WINDOW),
        }
    }
}

impl Limiter {
    /// Whether another attempt at `account` from `address` may go ahead.
    pub fn allows(&self, account: &str, address: Option<IpAddr>, now: Instant) -> bool {
        keys(account, address).all(|(key, limit)| self.failures.under(&key, limit, now))
    }

    pub fn record_failure(&self, account: &str, address: Option<IpAddr>, now: Instant) {
        for (key, _) in keys(account, address) {
            self.failures.record(key, now);
        }
    }

    /// Forgets the account's failures once its member signs in. The
    /// address's stay, so signing in to one account can't buy more guesses
    /// at others.
    pub fn record_success(&self, account: &str) {
        self.failures.forget(&account_key(account));
    }
}

/// What pages say when a limit is reached.
pub const TRY_LATER: &str = "Too many attempts. Try again in 15 minutes.";

/// Enough to try a few nicks while typing, too few to list the members.
pub const LOOKUPS_PER_ADDRESS: usize = 60;

/// Every load of the web client asks for a code and a token; this is
/// dozens of loads, far fewer than codes take to guess.
pub const OAUTH_REQUESTS_PER_ADDRESS: usize = 120;

/// Confirming takes one press per email; this leaves room for a
/// household's worth, and keeps scripted posts from hogging the database's
/// one writer.
pub const VERIFICATIONS_PER_ADDRESS: usize = 20;

/// Requests in the last [`WINDOW`], by client address, up to a limit.
pub struct AddressLimiter {
    requests: Recent<IpAddr>,
    limit: usize,
}

impl AddressLimiter {
    pub fn new(limit: usize) -> Self {
        Self {
            requests: Recent::new(WINDOW),
            limit,
        }
    }

    /// Whether another request from `address` may go ahead, counting it if so.
    pub fn allows(&self, address: Option<IpAddr>, now: Instant) -> bool {
        address.is_none_or(|address| self.requests.take(address, self.limit, now))
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
}

/// One verification link at a time: enough to find the first in a slow
/// inbox or the spam folder before asking again.
pub const LINK_INTERVAL: Duration = Duration::from_secs(5 * 60);
/// Enough for a few typos in an address, too few to flood a stranger who
/// was given as one.
pub const LINKS_PER_DAY: usize = 5;
/// Links mailed per hour, all members together: well under the mailer's
/// own cap, which reminders and other mail share.
pub const LINKS_PER_HOUR: usize = 60;
const DAY: Duration = Duration::from_secs(24 * 60 * 60);
const HOUR: Duration = Duration::from_secs(60 * 60);

/// Why a verification link wasn't mailed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MailRefusal {
    TooSoon,
    TooManyToday,
    Busy,
}

impl fmt::Display for MailRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::TooSoon => {
                "A link went out a few minutes ago. Give it time to arrive, and check your \
                 spam folder, before asking for another."
            }
            Self::TooManyToday => "That's all the links for today. Try again tomorrow.",
            Self::Busy => "A lot of mail is going out right now. Try again in an hour.",
        })
    }
}

/// Where a verification link goes, next to the member's last one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Recipient {
    /// The address the last link went to, which may still be on its way.
    SameAddress,
    /// An address just given, at signup or on the Account tab, which has
    /// no link yet: a member fixing a typo shouldn't have to wait.
    NewAddress,
}

/// Verification links mailed lately, by member and overall.
pub struct MailLimiter {
    recent: Recent<Key>,
    today: Recent<Key>,
    overall: Quota,
}

impl Default for MailLimiter {
    fn default() -> Self {
        Self {
            recent: Recent::new(LINK_INTERVAL),
            today: Recent::new(DAY),
            overall: Quota::new(LINKS_PER_HOUR, HOUR),
        }
    }
}

impl MailLimiter {
    /// Whether `account` may have a link mailed to `recipient` now.
    pub fn check(
        &self,
        account: &str,
        recipient: Recipient,
        now: Instant,
    ) -> Result<(), MailRefusal> {
        let key = account_key(account);
        if recipient == Recipient::SameAddress && !self.recent.under(&key, 1, now) {
            Err(MailRefusal::TooSoon)
        } else if !self.today.under(&key, LINKS_PER_DAY, now) {
            Err(MailRefusal::TooManyToday)
        } else if !self.overall.uses.under(&(), self.overall.limit, now) {
            Err(MailRefusal::Busy)
        } else {
            Ok(())
        }
    }

    /// Counts a link just mailed to `account`. Only links that went out
    /// count, so a failure to queue one never makes the member wait.
    pub fn record(&self, account: &str, now: Instant) {
        let key = account_key(account);
        self.recent.record(key.clone(), now);
        self.today.record(key, now);
        self.overall.uses.record((), now);
    }
}

fn account_key(account: &str) -> Key {
    Key::Account(account.to_ascii_lowercase())
}

fn keys(account: &str, address: Option<IpAddr>) -> impl Iterator<Item = (Key, usize)> {
    let address = address.map(|address| (Key::Address(address), PER_ADDRESS));
    [(account_key(account), PER_ACCOUNT)]
        .into_iter()
        .chain(address)
}

#[cfg(test)]
mod tests {
    use super::*;

    const HOME: Option<IpAddr> = Some(IpAddr::V4(std::net::Ipv4Addr::new(192, 0, 2, 7)));
    const CAFE: Option<IpAddr> = Some(IpAddr::V4(std::net::Ipv4Addr::new(198, 51, 100, 9)));

    fn fail(limiter: &Limiter, account: &str, address: Option<IpAddr>, times: usize, now: Instant) {
        for _ in 0..times {
            limiter.record_failure(account, address, now);
        }
    }

    #[test]
    fn the_address_comes_from_nginx_and_else_from_the_peer() {
        let peer: SocketAddr = "127.0.0.1:51234".parse().unwrap();
        let mut headers = HeaderMap::new();
        assert_eq!(ClientAddress::of(&headers, Some(peer)).0, Some(peer.ip()));
        assert_eq!(ClientAddress::of(&headers, None).0, None);
        headers.insert(REAL_IP, "2001:db8::7".parse().unwrap());
        assert_eq!(
            ClientAddress::of(&headers, Some(peer)).0,
            Some("2001:db8::7".parse().unwrap())
        );
        headers.insert(REAL_IP, "unknown".parse().unwrap());
        assert_eq!(ClientAddress::of(&headers, Some(peer)).0, None);
    }

    #[test]
    fn an_account_gets_five_tries_from_anywhere() {
        let (limiter, now) = (Limiter::default(), Instant::now());
        fail(&limiter, "alice", HOME, PER_ACCOUNT - 1, now);
        assert!(limiter.allows("alice", CAFE, now));
        fail(&limiter, "alice", CAFE, 1, now);
        assert!(!limiter.allows("alice", CAFE, now));
        assert!(!limiter.allows("ALICE", None, now));
        assert!(limiter.allows("bob", HOME, now));
    }

    #[test]
    fn an_address_gets_twenty_tries_across_accounts() {
        let (limiter, now) = (Limiter::default(), Instant::now());
        for n in 0..PER_ADDRESS - 1 {
            fail(&limiter, &format!("guess{n}"), HOME, 1, now);
        }
        assert!(limiter.allows("someone", HOME, now));
        fail(&limiter, "guess-last", HOME, 1, now);
        assert!(!limiter.allows("someone", HOME, now));
        assert!(limiter.allows("someone", CAFE, now));
    }

    #[test]
    fn failures_stop_counting_after_the_window() {
        let (limiter, start) = (Limiter::default(), Instant::now());
        fail(&limiter, "alice", HOME, PER_ACCOUNT, start);
        assert!(!limiter.allows("alice", HOME, start + WINDOW - Duration::from_secs(1)));
        assert!(limiter.allows("alice", HOME, start + WINDOW));
    }

    #[test]
    fn signing_in_clears_the_account_but_not_the_address() {
        let (limiter, now) = (Limiter::default(), Instant::now());
        fail(&limiter, "alice", HOME, PER_ACCOUNT, now);
        for n in 0..PER_ADDRESS - PER_ACCOUNT {
            fail(&limiter, &format!("guess{n}"), HOME, 1, now);
        }
        limiter.record_success("Alice");
        assert!(limiter.allows("alice", CAFE, now));
        assert!(!limiter.allows("alice", HOME, now));
    }

    #[test]
    fn without_an_address_only_the_account_counts() {
        let (limiter, now) = (Limiter::default(), Instant::now());
        for n in 0..PER_ADDRESS {
            fail(&limiter, &format!("guess{n}"), None, 1, now);
        }
        assert!(limiter.allows("someone", None, now));
    }

    #[test]
    fn an_address_gets_its_limit_of_requests_per_window() {
        let (lookups, start) = (AddressLimiter::new(LOOKUPS_PER_ADDRESS), Instant::now());
        for _ in 0..LOOKUPS_PER_ADDRESS {
            assert!(lookups.allows(HOME, start));
        }
        assert!(!lookups.allows(HOME, start));
        assert!(lookups.allows(CAFE, start));
        assert!(lookups.allows(None, start));
        assert!(lookups.allows(HOME, start + WINDOW));
    }

    #[test]
    fn a_quota_refills_as_uses_leave_the_window() {
        let (start, hour) = (Instant::now(), Duration::from_secs(3600));
        let quota = Quota::new(3, hour);
        let minute = |minutes: u64| start + Duration::from_secs(60 * minutes);
        for minutes in [0, 10, 20] {
            assert!(quota.take(minute(minutes)));
        }
        assert!(!quota.take(minute(59)));
        assert!(quota.take(minute(60)));
        assert!(!quota.take(minute(69)));
        assert!(quota.take(minute(70)));
    }

    fn mail(links: &MailLimiter, account: &str, now: Instant) -> Result<(), MailRefusal> {
        links.check(account, Recipient::SameAddress, now)?;
        links.record(account, now);
        Ok(())
    }

    #[test]
    fn checking_alone_counts_nothing() {
        let (links, now) = (MailLimiter::default(), Instant::now());
        for _ in 0..=LINKS_PER_DAY {
            assert_eq!(links.check("alice", Recipient::SameAddress, now), Ok(()));
        }
        assert_eq!(mail(&links, "alice", now), Ok(()));
        assert_eq!(
            links.check("alice", Recipient::SameAddress, now),
            Err(MailRefusal::TooSoon)
        );
    }

    #[test]
    fn a_member_gets_one_link_every_few_minutes() {
        let (links, start) = (MailLimiter::default(), Instant::now());
        assert_eq!(mail(&links, "alice", start), Ok(()));
        let soon = start + LINK_INTERVAL - Duration::from_secs(1);
        assert_eq!(mail(&links, "Alice", soon), Err(MailRefusal::TooSoon));
        assert_eq!(mail(&links, "bob", soon), Ok(()));
        assert_eq!(mail(&links, "alice", start + LINK_INTERVAL), Ok(()));
    }

    #[test]
    fn a_new_address_needs_no_wait_but_counts_toward_the_day() {
        let (links, now) = (MailLimiter::default(), Instant::now());
        for _ in 0..LINKS_PER_DAY {
            assert_eq!(links.check("alice", Recipient::NewAddress, now), Ok(()));
            links.record("alice", now);
        }
        assert_eq!(
            links.check("alice", Recipient::NewAddress, now),
            Err(MailRefusal::TooManyToday)
        );
    }

    #[test]
    fn a_member_gets_a_handful_of_links_a_day() {
        let (links, start) = (MailLimiter::default(), Instant::now());
        let every = |n: u32| start + LINK_INTERVAL * n;
        for n in 0..LINKS_PER_DAY as u32 {
            assert_eq!(mail(&links, "alice", every(n)), Ok(()));
        }
        let last = every(LINKS_PER_DAY as u32);
        assert_eq!(mail(&links, "alice", last), Err(MailRefusal::TooManyToday));
        assert_eq!(mail(&links, "alice", start + DAY), Ok(()));
    }

    #[test]
    fn all_members_together_get_a_quota_an_hour() {
        let (links, start) = (MailLimiter::default(), Instant::now());
        for n in 0..LINKS_PER_HOUR {
            assert_eq!(mail(&links, &format!("member{n}"), start), Ok(()));
        }
        assert_eq!(mail(&links, "alice", start), Err(MailRefusal::Busy));
        let next = start + Duration::from_secs(1);
        assert_eq!(mail(&links, "alice", next), Err(MailRefusal::Busy));
        assert_eq!(mail(&links, "alice", start + HOUR), Ok(()));
        assert_eq!(mail(&links, "member0", start + HOUR), Ok(()));
    }

    #[test]
    fn forgets_keys_without_recent_failures_once_it_tracks_many() {
        let (limiter, start) = (Limiter::default(), Instant::now());
        for n in 0..=PRUNE_ABOVE {
            fail(&limiter, &format!("guess{n}"), None, 1, start);
        }
        fail(&limiter, "alice", HOME, 1, start + WINDOW);
        assert_eq!(limiter.failures.len(), 2);
    }
}
