//! Slows down password guessing on the website. Ergo doesn't throttle
//! `check_auth`, so this is the only guard; it lives in memory, and failed
//! website attempts never lock anyone out of IRC.

use std::{
    collections::HashMap,
    fmt,
    net::{IpAddr, SocketAddr},
    sync::{Mutex, MutexGuard},
    time::{Duration, Instant},
};

use axum::{
    extract::{ConnectInfo, FromRequestParts},
    http::{HeaderMap, request::Parts},
};
use common::recent::{Quota, Recent};

use crate::{name::MemberName, upload_rules};

/// How long a failed attempt counts against its account and address.
pub const WINDOW: Duration = Duration::from_secs(15 * 60);
/// Enough for a member fumbling their own password a few times.
pub const PER_ACCOUNT: usize = 5;
/// Enough for a household or an office behind one address.
pub const PER_ADDRESS: usize = 20;

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

/// Asking for a reset link takes a post, and setting the password another,
/// or a few for one the rules refuse; this leaves room for a household,
/// and keeps one address from trying a list of nicks and addresses.
pub const RECOVERIES_PER_ADDRESS: usize = 20;

/// Making, resetting or deleting a bot has Ergo hash a password or erase
/// an account, and a made bot keeps its name from others even past its
/// deletion; this leaves room to set up a member's bots and fix a few
/// names, and keeps one address from taking names by the hundred.
pub const BOT_CHANGES_PER_ADDRESS: usize = 10;

/// Uploads stored in the last hour, by the account that made them: a
/// bot's apart from its owner's.
pub struct UploadLimiter {
    stored: Recent<String>,
}

impl Default for UploadLimiter {
    fn default() -> Self {
        Self {
            stored: Recent::new(HOUR),
        }
    }
}

impl UploadLimiter {
    /// Counts an upload `account` is about to make, if the hour has room
    /// for it, so uploads at once can't all take the last of it. The count
    /// is given back unless the upload is stored and its reservation kept.
    pub fn reserve(&self, account: &str, now: Instant) -> Option<Reservation<'_>> {
        let account = account.to_ascii_lowercase();
        let taken = self
            .stored
            .take(account.clone(), upload_rules::PER_HOUR, now);
        taken.then(|| Reservation {
            limiter: self,
            account,
            at: now,
            kept: false,
        })
    }
}

/// An upload counted against its account's hour, until it's dropped
/// without being kept.
pub struct Reservation<'a> {
    limiter: &'a UploadLimiter,
    account: String,
    at: Instant,
    kept: bool,
}

impl Reservation<'_> {
    /// Keeps the upload counted: it was stored.
    pub fn keep(mut self) {
        self.kept = true;
    }
}

impl Drop for Reservation<'_> {
    fn drop(&mut self) {
        if !self.kept {
            self.limiter.stored.forget_one(&self.account, self.at);
        }
    }
}

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

/// One link at a time, to confirm an email or reset a password: enough to
/// find the first in a slow inbox or the spam folder before asking again.
pub const LINK_INTERVAL: Duration = Duration::from_secs(5 * 60);
/// Enough for a few typos in an address, too few to flood a stranger who
/// was given as one, or a member whose nick someone keeps typing in.
pub const LINKS_PER_DAY: usize = 5;
/// Links of one kind mailed per hour, all members together: verification
/// and reset links, and a batch of reminders, stay well under the
/// mailer's own cap of 400.
pub const LINKS_PER_HOUR: usize = 60;
const DAY: Duration = Duration::from_secs(24 * 60 * 60);
const HOUR: Duration = Duration::from_secs(60 * 60);

/// Why a link wasn't mailed.
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

/// Where a link goes, next to the member's last one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Recipient {
    /// The address the last link went to, which may still be on its way.
    SameAddress,
    /// An address just given, at signup or on the Account tab, which has
    /// no link yet: a member fixing a typo shouldn't have to wait.
    NewAddress,
}

/// Links of one kind mailed lately, by member and overall.
pub struct MailLimiter {
    recent: Recent<MemberName>,
    today: Recent<MemberName>,
    overall: Quota,
    /// Whose link each message queued in the last day carries, and when it
    /// was queued, by message id.
    links: Mutex<HashMap<i64, (MemberName, Instant)>>,
}

impl Default for MailLimiter {
    fn default() -> Self {
        Self {
            recent: Recent::new(LINK_INTERVAL),
            today: Recent::new(DAY),
            overall: Quota::new(LINKS_PER_HOUR, HOUR),
            links: Mutex::default(),
        }
    }
}

impl MailLimiter {
    /// Whether `account` may have a link mailed to `recipient` now.
    pub fn check(
        &self,
        account: &MemberName,
        recipient: Recipient,
        now: Instant,
    ) -> Result<(), MailRefusal> {
        if recipient == Recipient::SameAddress && !self.recent.under(account, 1, now) {
            Err(MailRefusal::TooSoon)
        } else if !self.today.under(account, LINKS_PER_DAY, now) {
            Err(MailRefusal::TooManyToday)
        } else if self.busy(now) {
            Err(MailRefusal::Busy)
        } else {
            Ok(())
        }
    }

    /// Whether every member's links together have reached the hourly cap.
    pub fn busy(&self, now: Instant) -> bool {
        !self.overall.has_room(now)
    }

    /// Counts the link to `account` just queued as message `id`. Only links
    /// that went out count, so a failure to queue one never makes the
    /// member wait.
    pub fn record(&self, account: &MemberName, id: i64, now: Instant) {
        self.recent.record(account.clone(), now);
        self.today.record(account.clone(), now);
        self.overall.record(now);
        let mut links = self.links();
        links.retain(|_, (_, queued_at)| now.duration_since(*queued_at) < DAY);
        links.insert(id, (account.clone(), now));
    }

    /// Stops counting the links in messages `ids`, which the mailer deleted
    /// unsent.
    pub fn refund(&self, ids: &[i64]) {
        let mut links = self.links();
        for id in ids {
            if let Some((account, queued_at)) = links.remove(id) {
                self.recent.forget_one(&account, queued_at);
                self.today.forget_one(&account, queued_at);
                self.overall.forget_one(queued_at);
            }
        }
    }

    fn links(&self) -> MutexGuard<'_, HashMap<i64, (MemberName, Instant)>> {
        self.links
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
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
    fn an_account_stores_its_hourly_uploads_whatever_its_owner_does() {
        let (uploads, start) = (UploadLimiter::default(), Instant::now());
        for _ in 0..upload_rules::PER_HOUR {
            uploads.reserve("Marvin", start).unwrap().keep();
        }
        assert!(uploads.reserve("marvin", start).is_none());
        assert!(uploads.reserve("alice", start).is_some());
        let almost = start + HOUR - Duration::from_secs(1);
        assert!(uploads.reserve("MARVIN", almost).is_none());
        assert!(uploads.reserve("marvin", start + HOUR).is_some());
    }

    #[test]
    fn uploads_under_way_hold_their_place_and_refused_ones_give_it_back() {
        let (uploads, now) = (UploadLimiter::default(), Instant::now());
        let under_way: Vec<_> = (0..upload_rules::PER_HOUR)
            .map(|_| uploads.reserve("alice", now).unwrap())
            .collect();
        assert!(uploads.reserve("alice", now).is_none());
        drop(under_way);
        assert!(uploads.reserve("alice", now).is_some());
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

    /// Message ids, as the outbox gives each message its own.
    static NEXT_ID: std::sync::atomic::AtomicI64 = std::sync::atomic::AtomicI64::new(1);

    /// Mails `account` a link if the limits allow, answering its message id.
    fn mail(links: &MailLimiter, account: &str, now: Instant) -> Result<i64, MailRefusal> {
        links.check(&MemberName::new(account), Recipient::SameAddress, now)?;
        let id = NEXT_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        links.record(&MemberName::new(account), id, now);
        Ok(id)
    }

    #[test]
    fn checking_alone_counts_nothing() {
        let (links, now) = (MailLimiter::default(), Instant::now());
        for _ in 0..=LINKS_PER_DAY {
            assert_eq!(
                links.check(&MemberName::new("alice"), Recipient::SameAddress, now),
                Ok(())
            );
        }
        assert!(mail(&links, "alice", now).is_ok());
        assert_eq!(
            links.check(&MemberName::new("alice"), Recipient::SameAddress, now),
            Err(MailRefusal::TooSoon)
        );
    }

    #[test]
    fn a_member_gets_one_link_every_few_minutes() {
        let (links, start) = (MailLimiter::default(), Instant::now());
        assert!(mail(&links, "alice", start).is_ok());
        let soon = start + LINK_INTERVAL - Duration::from_secs(1);
        assert_eq!(mail(&links, "Alice", soon), Err(MailRefusal::TooSoon));
        assert!(mail(&links, "bob", soon).is_ok());
        assert!(mail(&links, "alice", start + LINK_INTERVAL).is_ok());
    }

    #[test]
    fn a_new_address_needs_no_wait_but_counts_toward_the_day() {
        let (links, now) = (MailLimiter::default(), Instant::now());
        for id in 0..LINKS_PER_DAY as i64 {
            assert_eq!(
                links.check(&MemberName::new("alice"), Recipient::NewAddress, now),
                Ok(())
            );
            links.record(&MemberName::new("alice"), id, now);
        }
        assert_eq!(
            links.check(&MemberName::new("alice"), Recipient::NewAddress, now),
            Err(MailRefusal::TooManyToday)
        );
    }

    #[test]
    fn a_member_gets_a_handful_of_links_a_day() {
        let (links, start) = (MailLimiter::default(), Instant::now());
        let every = |n: u32| start + LINK_INTERVAL * n;
        for n in 0..LINKS_PER_DAY as u32 {
            assert!(mail(&links, "alice", every(n)).is_ok());
        }
        let last = every(LINKS_PER_DAY as u32);
        assert_eq!(mail(&links, "alice", last), Err(MailRefusal::TooManyToday));
        assert!(mail(&links, "alice", start + DAY).is_ok());
    }

    #[test]
    fn all_members_together_get_a_quota_an_hour() {
        let (links, start) = (MailLimiter::default(), Instant::now());
        for n in 0..LINKS_PER_HOUR {
            assert!(mail(&links, &format!("member{n}"), start).is_ok());
        }
        assert_eq!(mail(&links, "alice", start), Err(MailRefusal::Busy));
        assert!(links.busy(start));
        let next = start + Duration::from_secs(1);
        assert_eq!(mail(&links, "alice", next), Err(MailRefusal::Busy));
        assert!(!links.busy(start + HOUR));
        assert!(mail(&links, "alice", start + HOUR).is_ok());
        assert!(mail(&links, "member0", start + HOUR).is_ok());
    }

    #[test]
    fn a_link_the_mailer_deleted_unsent_counts_against_nothing() {
        let (links, start) = (MailLimiter::default(), Instant::now());
        let busy: Vec<_> = (1..LINKS_PER_HOUR)
            .map(|n| mail(&links, &format!("member{n}"), start).unwrap())
            .collect();
        let mut alices = Vec::new();
        for _ in 0..2 * LINKS_PER_DAY {
            alices.push(mail(&links, "alice", start).unwrap());
            assert_eq!(mail(&links, "alice", start), Err(MailRefusal::TooSoon));
            links.refund(&alices[alices.len() - 1..]);
        }
        assert!(mail(&links, "alice", start).is_ok());
        assert_eq!(mail(&links, "bob", start), Err(MailRefusal::Busy));

        links.refund(&busy[..1]);
        links.refund(&busy[..1]);
        links.refund(&[0, i64::MAX]);

        assert!(mail(&links, "bob", start).is_ok());
        assert_eq!(mail(&links, "carol", start), Err(MailRefusal::Busy));
    }

    #[test]
    fn a_link_is_given_back_only_within_the_day_it_counts() {
        let (links, start) = (MailLimiter::default(), Instant::now());
        let first = mail(&links, "alice", start).unwrap();
        let later = start + DAY;
        let second = mail(&links, "alice", later).unwrap();

        links.refund(&[first]);
        assert_eq!(mail(&links, "alice", later), Err(MailRefusal::TooSoon));
        links.refund(&[second]);

        assert!(mail(&links, "alice", later).is_ok());
    }
}
