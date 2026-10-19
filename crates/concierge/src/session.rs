//! Sign-in sessions: a random token in a cookie, and only its keyed hash in
//! the database, so a copy of the database signs nobody in. The web
//! client's OAuth codes and tokens are tokens of the same kind.
//!
//! Times are Unix seconds.

use axum::http::{HeaderMap, HeaderValue, header};
use common::random;
use hmac::{Hmac, Mac};
use sha2::Sha256;

use crate::secret_hash::{Keyed, SecretHash};

/// `__Host-` makes browsers insist on Secure, Path=/ and no Domain, so no
/// other subdomain can plant or read it. Browsers treat http://localhost as
/// secure, so the dev loop gets the same cookie as production.
pub const COOKIE: &str = "__Host-session";
const DAY: i64 = 24 * 60 * 60;
pub const LIFETIME: i64 = 30 * DAY;
/// How often a session in use gets its expiry pushed back, so not every
/// request writes to the database.
const RENEWAL_INTERVAL: i64 = DAY;
/// 43 base64url characters: 258 bits.
const TOKEN_LENGTH: usize = 43;

pub fn expires_at(now: i64) -> i64 {
    now + LIFETIME
}

/// Whether a session last renewed (or created) to expire at `expires_at`
/// is due for renewal.
pub fn needs_renewal(expires_at: i64, now: i64) -> bool {
    expires_at - now <= LIFETIME - RENEWAL_INTERVAL
}

#[derive(Clone, PartialEq, Eq)]
pub struct Token(String);

impl Token {
    pub fn generate() -> Self {
        Self(random::string(random::URL_SAFE, TOKEN_LENGTH))
    }

    /// The token in the request's session cookie, if it holds one that
    /// could have been generated here.
    pub fn from_cookies(headers: &HeaderMap) -> Option<Self> {
        headers
            .get_all(header::COOKIE)
            .iter()
            .filter_map(|value| value.to_str().ok())
            .flat_map(|cookies| cookies.split(';'))
            .filter_map(|cookie| cookie.trim().strip_prefix(COOKIE)?.strip_prefix('='))
            .find_map(Self::parse)
    }

    /// `value` as a token, if it could have been generated here.
    pub fn parse(value: &str) -> Option<Self> {
        let plausible =
            value.len() == TOKEN_LENGTH && value.bytes().all(|c| random::URL_SAFE.contains(&c));
        plausible.then(|| Self(value.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// A `Set-Cookie` value that keeps the token for [`LIFETIME`].
    pub fn cookie(&self) -> HeaderValue {
        cookie(&self.0, LIFETIME)
    }
}

/// A `Set-Cookie` value that makes the browser forget its session.
pub fn removal_cookie() -> HeaderValue {
    cookie("", 0)
}

fn cookie(value: &str, max_age: i64) -> HeaderValue {
    format!("{COOKIE}={value}; Path=/; Max-Age={max_age}; HttpOnly; Secure; SameSite=Lax")
        .parse()
        .expect("tokens are base64url")
}

/// Hashes tokens with the machine's session key.
#[derive(Clone)]
pub struct Hasher(Hmac<Sha256>);

impl Hasher {
    pub fn new(key: &[u8]) -> Self {
        Self(Hmac::new_from_slice(key).expect("HMAC takes keys of any length"))
    }

    /// What the database keeps of `token`, used as a secret of the kind
    /// `K`.
    pub fn hash<K: Keyed>(&self, token: &Token) -> SecretHash<K> {
        let mut mac = self.0.clone();
        mac.update(token.0.as_bytes());
        SecretHash::from_bytes(mac.finalize().into_bytes().to_vec())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::secret_hash::Session;

    const NOW: i64 = 1_760_000_000;

    fn headers(cookies: &[&str]) -> HeaderMap {
        let mut headers = HeaderMap::new();
        for value in cookies {
            headers.append(header::COOKIE, value.parse().unwrap());
        }
        headers
    }

    #[test]
    fn tokens_carry_at_least_256_bits_and_differ() {
        assert!(random::entropy_bits(random::URL_SAFE, TOKEN_LENGTH) >= 256.0);
        assert!(Token::generate() != Token::generate());
    }

    #[test]
    fn reads_the_token_among_other_cookies() {
        let token = Token::generate();
        let found = Token::from_cookies(&headers(&[&format!(
            "theme=dark; {COOKIE}={}; lang=en",
            token.0
        )]));
        assert!(found == Some(token));
    }

    #[test]
    fn reads_the_token_from_any_cookie_header() {
        let token = Token::generate();
        let found =
            Token::from_cookies(&headers(&["theme=dark", &format!("{COOKIE}={}", token.0)]));
        assert!(found == Some(token));
    }

    #[test]
    fn ignores_values_no_token_could_have() {
        let token = Token::generate().0;
        for cookie in [
            String::new(),
            format!("session={token}"),
            format!("{COOKIE}x={token}"),
            format!("{COOKIE}={}", &token[1..]),
            format!("{COOKIE}={token}A"),
            format!("{COOKIE}={}.", &token[1..]),
        ] {
            assert!(
                Token::from_cookies(&headers(&[&cookie])).is_none(),
                "{cookie}"
            );
        }
    }

    #[test]
    fn parses_only_what_could_be_a_token() {
        let token = Token::generate();
        assert!(Token::parse(token.as_str()) == Some(token.clone()));
        for value in ["", &token.0[1..], &format!("{}=", &token.0[1..])] {
            assert!(Token::parse(value).is_none(), "{value}");
        }
    }

    #[test]
    fn the_cookie_is_host_only_http_only_secure_and_lax() {
        let token = Token::generate();
        assert_eq!(
            token.cookie().to_str().unwrap(),
            format!(
                "__Host-session={}; Path=/; Max-Age=2592000; HttpOnly; Secure; SameSite=Lax",
                token.0
            )
        );
        assert_eq!(
            removal_cookie().to_str().unwrap(),
            "__Host-session=; Path=/; Max-Age=0; HttpOnly; Secure; SameSite=Lax"
        );
    }

    #[test]
    fn hashes_depend_on_the_token_and_the_key() {
        let (token, other) = (Token::generate(), Token::generate());
        let hash = |hasher: &Hasher, token| hasher.hash::<Session>(token);
        let hasher = Hasher::new(b"key one");
        assert_eq!(hash(&hasher, &token), hash(&hasher, &token));
        assert_eq!(hash(&hasher, &token).as_bytes().len(), 32);
        assert_ne!(hash(&hasher, &token), hash(&hasher, &other));
        assert_ne!(
            hash(&hasher, &token),
            hash(&Hasher::new(b"key two"), &token)
        );
    }

    #[test]
    fn sessions_last_thirty_days() {
        assert_eq!(expires_at(NOW), NOW + 30 * DAY);
    }

    #[test]
    fn a_session_is_renewed_once_a_day_at_most() {
        let fresh = expires_at(NOW);
        assert!(!needs_renewal(fresh, NOW));
        assert!(!needs_renewal(fresh, NOW + DAY - 1));
        assert!(needs_renewal(fresh, NOW + DAY));
        assert!(needs_renewal(fresh, NOW + 29 * DAY));
    }
}
