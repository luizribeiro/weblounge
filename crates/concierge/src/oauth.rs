//! Concierge as a small OAuth 2.0 authorization server: the web client
//! signs members in with an authorization code and PKCE, and Ergo checks
//! the tokens it hands over with introspection.
//!
//! Times are Unix seconds.

use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;

use crate::db::AuthorizationCode;

/// The web client, gamja: public, so it has no secret, and PKCE stands in.
pub const WEBCHAT_CLIENT: &str = "gamja";
/// Ergo, the only client allowed to introspect tokens.
pub const INTROSPECTION_CLIENT: &str = "ergo";
/// Long enough for a browser to follow a redirect and post the code back.
pub const CODE_LIFETIME: i64 = 60;
/// SHA-256 in unpadded base64url.
const CHALLENGE_LENGTH: usize = 43;
/// RFC 7636's bounds on a code verifier.
const VERIFIER_LENGTHS: std::ops::RangeInclusive<usize> = 43..=128;
/// Generous for any random state a client keeps; gamja's is 43 characters.
/// RFC 6749 allows printable ASCII in it.
const MAX_STATE_LENGTH: usize = 256;

/// The one registered client and where it may be sent back to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Webchat {
    pub origin: String,
}

impl Webchat {
    /// gamja sends its own address, which is its origin's root.
    pub fn redirect_uri(&self) -> String {
        format!("{}/", self.origin)
    }

    /// Where members open the chat, as they'd type it.
    pub fn address(&self) -> &str {
        self.origin
            .split_once("://")
            .map_or(&self.origin, |(_, address)| address)
    }
}

/// The query of a request to the authorization endpoint.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct AuthorizationRequest {
    pub response_type: Option<String>,
    pub client_id: Option<String>,
    pub redirect_uri: Option<String>,
    pub state: Option<String>,
    pub code_challenge: Option<String>,
    pub code_challenge_method: Option<String>,
}

/// An authorization request worth issuing a code for.
#[derive(Debug, PartialEq, Eq)]
pub struct Authorization {
    pub redirect_uri: String,
    pub state: String,
    pub code_challenge: CodeChallenge,
}

/// Why an authorization request was refused.
#[derive(Debug, PartialEq, Eq)]
pub enum Refusal {
    /// The client or its redirect URI isn't ours, so the browser must not
    /// be sent there; the member gets an error page instead.
    UnknownClient,
    /// The client is ours and gets the error at its redirect URI.
    Invalid {
        redirect_uri: String,
        state: Option<String>,
        error: &'static str,
    },
}

impl AuthorizationRequest {
    pub fn check(self, webchat: &Webchat) -> Result<Authorization, Refusal> {
        let redirect_uri = webchat.redirect_uri();
        if self.client_id.as_deref() != Some(WEBCHAT_CLIENT)
            || self.redirect_uri.as_deref() != Some(&redirect_uri)
        {
            return Err(Refusal::UnknownClient);
        }
        let state = self
            .state
            .filter(|state| (1..=MAX_STATE_LENGTH).contains(&state.len()))
            .filter(|state| state.bytes().all(|c| (0x20..=0x7e).contains(&c)));
        let invalid = |error| Refusal::Invalid {
            redirect_uri: redirect_uri.clone(),
            state: state.clone(),
            error,
        };
        if self.response_type.as_deref() != Some("code") {
            return Err(invalid("unsupported_response_type"));
        }
        let code_challenge = self
            .code_challenge
            .as_deref()
            .and_then(CodeChallenge::parse)
            .filter(|_| self.code_challenge_method.as_deref() == Some("S256"))
            .ok_or_else(|| invalid("invalid_request"))?;
        let state = state.clone().ok_or_else(|| invalid("invalid_request"))?;
        Ok(Authorization {
            redirect_uri,
            state,
            code_challenge,
        })
    }
}

impl Refusal {
    /// Where to send the browser with the error, if anywhere.
    pub fn redirect(&self) -> Option<String> {
        let Self::Invalid {
            redirect_uri,
            state,
            error,
        } = self
        else {
            return None;
        };
        let mut query = form_urlencoded::Serializer::new(String::new());
        query.append_pair("error", error);
        if let Some(state) = state {
            query.append_pair("state", state);
        }
        Some(format!("{redirect_uri}?{}", query.finish()))
    }
}

impl Authorization {
    /// Where to send the browser when the request was fine but no code can
    /// be issued for it.
    pub fn error_redirect(&self, error: &'static str) -> String {
        let refusal = Refusal::Invalid {
            redirect_uri: self.redirect_uri.clone(),
            state: Some(self.state.clone()),
            error,
        };
        refusal
            .redirect()
            .expect("our own client has a redirect URI")
    }
}

/// Where the browser takes the code back to the client.
pub fn code_redirect(authorization: &Authorization, code: &str) -> String {
    let query = form_urlencoded::Serializer::new(String::new())
        .append_pair("code", code)
        .append_pair("state", &authorization.state)
        .finish();
    format!("{}?{query}", authorization.redirect_uri)
}

/// The form the web client posts to the token endpoint.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct TokenRequest {
    pub grant_type: String,
    pub code: String,
    pub redirect_uri: String,
    pub client_id: String,
    pub code_verifier: String,
}

impl TokenRequest {
    /// Whether `code`, just taken from the database, was issued for this
    /// very client, redirect URI and verifier, and hasn't expired.
    pub fn redeems(&self, code: &AuthorizationCode, now: i64) -> bool {
        code.expires_at > now
            && code.client_id == self.client_id
            && code.redirect_uri == self.redirect_uri
            && CodeVerifier::parse(&self.code_verifier)
                .is_some_and(|verifier| verifies(&verifier, &code.code_challenge))
    }
}

/// An S256 code challenge: public, since it travels in the authorization URL.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodeChallenge(String);

impl CodeChallenge {
    /// `value` as a challenge, if it could be a SHA-256 in unpadded base64url.
    pub fn parse(value: &str) -> Option<Self> {
        let well_formed = value.len() == CHALLENGE_LENGTH
            && value
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_');
        well_formed.then(|| Self(value.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// The secret a client proves it started an authorization with. No Debug,
/// so it stays out of logs.
pub struct CodeVerifier(String);

impl CodeVerifier {
    /// `value` as a verifier, if RFC 7636 allows it.
    pub fn parse(value: &str) -> Option<Self> {
        let well_formed = VERIFIER_LENGTHS.contains(&value.len())
            && value
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || b"-._~".contains(&c));
        well_formed.then(|| Self(value.to_owned()))
    }
}

/// Whether `verifier` is the one `challenge` was made from, by S256.
pub fn verifies(verifier: &CodeVerifier, challenge: &CodeChallenge) -> bool {
    let derived = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.0.as_bytes()));
    derived.as_bytes().ct_eq(challenge.0.as_bytes()).into()
}

/// Whether an `Authorization` header authenticates Ergo with `secret`,
/// by HTTP Basic with form-encoded credentials, as RFC 6749 has it.
pub fn authenticates_introspection(authorization: Option<&str>, secret: &str) -> bool {
    let Some((id, given)) = authorization.and_then(basic_credentials) else {
        return false;
    };
    let right_secret: bool = given.as_bytes().ct_eq(secret.as_bytes()).into();
    id == INTROSPECTION_CLIENT && right_secret && !secret.is_empty()
}

fn basic_credentials(authorization: &str) -> Option<(String, String)> {
    let (scheme, encoded) = authorization.split_once(' ')?;
    if !scheme.eq_ignore_ascii_case("basic") {
        return None;
    }
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(encoded.trim())
        .ok()?;
    let decoded = String::from_utf8(decoded).ok()?;
    let (id, secret) = decoded.split_once(':')?;
    let unescape = |part: &str| {
        form_urlencoded::parse(format!("x={part}").as_bytes())
            .next()
            .map(|(_, value)| value.into_owned())
    };
    Some((unescape(id)?, unescape(secret)?))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// RFC 7636, appendix B.
    const VERIFIER: &str = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";
    const CHALLENGE: &str = "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM";
    const SECRET: &str = "9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08";

    fn challenge() -> CodeChallenge {
        CodeChallenge::parse(CHALLENGE).unwrap()
    }

    fn s256(verifier: &str) -> CodeChallenge {
        CodeChallenge::parse(&URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))).unwrap()
    }

    fn webchat() -> Webchat {
        Webchat {
            origin: "https://irc.weblounge.org".into(),
        }
    }

    /// What gamja sends.
    fn gamja() -> AuthorizationRequest {
        AuthorizationRequest {
            response_type: Some("code".into()),
            client_id: Some("gamja".into()),
            redirect_uri: Some("https://irc.weblounge.org/".into()),
            state: Some("q7Hx2mZp9vLcT4wRfBa-_wq7Hx2mZp9vLcT4wRfBa-_".into()),
            code_challenge: Some(CHALLENGE.into()),
            code_challenge_method: Some("S256".into()),
        }
    }

    fn basic(credentials: &str) -> String {
        format!(
            "Basic {}",
            base64::engine::general_purpose::STANDARD.encode(credentials)
        )
    }

    #[test]
    fn accepts_gamjas_request() {
        assert_eq!(
            gamja().check(&webchat()),
            Ok(Authorization {
                redirect_uri: "https://irc.weblounge.org/".into(),
                state: gamja().state.unwrap(),
                code_challenge: challenge(),
            })
        );
    }

    #[test]
    fn the_redirect_uri_must_match_exactly() {
        for uri in [
            "https://irc.weblounge.org",
            "https://irc.weblounge.org/#lounge",
            "https://irc.weblounge.org/?x=1",
            "https://irc.weblounge.org//",
            "http://irc.weblounge.org/",
            "https://irc.weblounge.org:8443/",
            "https://IRC.weblounge.org/",
            "https://irc.weblounge.org.evil.example/",
            "https://evil.example/",
        ] {
            let request = AuthorizationRequest {
                redirect_uri: Some(uri.into()),
                ..gamja()
            };
            assert_eq!(
                request.check(&webchat()),
                Err(Refusal::UnknownClient),
                "{uri}"
            );
        }
        let request = AuthorizationRequest {
            redirect_uri: None,
            ..gamja()
        };
        assert_eq!(request.check(&webchat()), Err(Refusal::UnknownClient));
    }

    #[test]
    fn other_clients_get_no_redirect() {
        for client_id in [None, Some("ergo"), Some("Gamja"), Some("")] {
            let request = AuthorizationRequest {
                client_id: client_id.map(Into::into),
                ..gamja()
            };
            let refusal = request.check(&webchat()).unwrap_err();
            assert_eq!(refusal, Refusal::UnknownClient, "{client_id:?}");
            assert_eq!(refusal.redirect(), None);
        }
    }

    #[test]
    fn the_address_is_the_origin_without_its_scheme() {
        assert_eq!(webchat().address(), "irc.weblounge.org");
        let vm = Webchat {
            origin: "https://irc.weblounge.localhost:10443".into(),
        };
        assert_eq!(vm.address(), "irc.weblounge.localhost:10443");
    }

    #[test]
    fn a_vm_redirect_carries_the_forwarded_port() {
        let vm = Webchat {
            origin: "https://irc.weblounge.localhost:10443".into(),
        };
        let request = AuthorizationRequest {
            redirect_uri: Some("https://irc.weblounge.localhost:10443/".into()),
            ..gamja()
        };
        assert!(request.check(&vm).is_ok());
        assert_eq!(gamja().check(&vm), Err(Refusal::UnknownClient));
    }

    fn refused_with(request: AuthorizationRequest) -> Option<String> {
        request.check(&webchat()).unwrap_err().redirect()
    }

    #[test]
    fn requests_without_pkce_s256_are_sent_back_with_an_error() {
        let state = gamja().state.unwrap();
        let back = format!("https://irc.weblounge.org/?error=invalid_request&state={state}");
        for (challenge, method) in [
            (None, Some("S256")),
            (Some(CHALLENGE), None),
            (Some(CHALLENGE), Some("plain")),
            (Some(VERIFIER), Some("s256")),
            (Some(&CHALLENGE[1..]), Some("S256")),
            (
                Some("E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw+cM"),
                Some("S256"),
            ),
        ] {
            let request = AuthorizationRequest {
                code_challenge: challenge.map(Into::into),
                code_challenge_method: method.map(Into::into),
                ..gamja()
            };
            assert_eq!(
                refused_with(request),
                Some(back.clone()),
                "{challenge:?} {method:?}"
            );
        }
    }

    #[test]
    fn requests_without_a_state_are_refused() {
        for state in [
            None,
            Some(String::new()),
            Some("s".repeat(MAX_STATE_LENGTH + 1)),
            Some("line\r\nbreak".into()),
            Some("caf\u{e9}".into()),
        ] {
            let request = AuthorizationRequest { state, ..gamja() };
            assert_eq!(
                refused_with(request).as_deref(),
                Some("https://irc.weblounge.org/?error=invalid_request")
            );
        }
    }

    #[test]
    fn only_the_code_flow_is_supported() {
        let request = AuthorizationRequest {
            response_type: Some("token".into()),
            state: Some("a b&c".into()),
            ..gamja()
        };
        assert_eq!(
            refused_with(request).as_deref(),
            Some("https://irc.weblounge.org/?error=unsupported_response_type&state=a+b%26c")
        );
    }

    #[test]
    fn the_code_goes_back_with_the_state() {
        let authorization = gamja().check(&webchat()).unwrap();
        assert_eq!(
            code_redirect(&authorization, "c0de"),
            format!(
                "https://irc.weblounge.org/?code=c0de&state={}",
                authorization.state
            )
        );
    }

    #[test]
    fn a_valid_request_can_still_be_refused_back_to_the_client() {
        let authorization = gamja().check(&webchat()).unwrap();
        assert_eq!(
            authorization.error_redirect("temporarily_unavailable"),
            format!(
                "https://irc.weblounge.org/?error=temporarily_unavailable&state={}",
                authorization.state
            )
        );
    }

    const ISSUED: i64 = 1_760_000_000;

    fn issued_code() -> AuthorizationCode {
        AuthorizationCode {
            session_hash: vec![0xa1],
            client_id: "gamja".into(),
            redirect_uri: "https://irc.weblounge.org/".into(),
            code_challenge: challenge(),
            expires_at: ISSUED + CODE_LIFETIME,
        }
    }

    fn exchange() -> TokenRequest {
        TokenRequest {
            grant_type: "authorization_code".into(),
            code: "c0de".into(),
            redirect_uri: "https://irc.weblounge.org/".into(),
            client_id: "gamja".into(),
            code_verifier: VERIFIER.into(),
        }
    }

    #[test]
    fn a_code_is_redeemed_by_its_client_until_it_expires() {
        let code = issued_code();
        assert!(exchange().redeems(&code, ISSUED));
        assert!(exchange().redeems(&code, ISSUED + CODE_LIFETIME - 1));
        assert!(!exchange().redeems(&code, ISSUED + CODE_LIFETIME));
    }

    #[test]
    fn a_code_is_bound_to_its_client_redirect_and_challenge() {
        let code = issued_code();
        for request in [
            TokenRequest {
                client_id: "ergo".into(),
                ..exchange()
            },
            TokenRequest {
                redirect_uri: "https://evil.example/".into(),
                ..exchange()
            },
            TokenRequest {
                code_verifier: CHALLENGE.into(),
                ..exchange()
            },
            TokenRequest {
                code_verifier: String::new(),
                ..exchange()
            },
        ] {
            assert!(!request.redeems(&code, ISSUED), "{request:?}");
        }
    }

    #[test]
    fn a_challenge_is_43_characters_of_base64url() {
        let alphabet = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
        for value in [CHALLENGE, &alphabet[..43], &alphabet[21..]] {
            assert_eq!(CodeChallenge::parse(value).map(|c| c.0), Some(value.into()));
        }
        for value in [
            "",
            &CHALLENGE[1..],
            &format!("{CHALLENGE}A"),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw+cM",
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw/cM",
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-c=",
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw.cM",
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw~cM",
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSst\u{e9}cM",
        ] {
            assert_eq!(CodeChallenge::parse(value), None, "{value}");
        }
    }

    fn verifier(value: &str) -> CodeVerifier {
        CodeVerifier::parse(value).unwrap()
    }

    #[test]
    fn a_verifier_is_43_to_128_unreserved_characters() {
        let unreserved = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-._~";
        for value in [VERIFIER, unreserved, &unreserved[..43], &"~".repeat(128)] {
            assert!(
                CodeVerifier::parse(value).is_some_and(|v| v.0 == value),
                "{value}"
            );
        }
        for value in [
            "",
            &"a".repeat(42),
            &"a".repeat(129),
            &format!("{} ", "a".repeat(43)),
            &format!("{}+", "a".repeat(43)),
            &format!("{}/", "a".repeat(43)),
            &format!("{}=", "a".repeat(43)),
            &format!("{}%7E", "a".repeat(43)),
            &format!("{}\u{e9}", "a".repeat(43)),
        ] {
            assert!(CodeVerifier::parse(value).is_none(), "{value}");
        }
    }

    #[test]
    fn verifies_the_rfc_example() {
        assert!(verifies(&verifier(VERIFIER), &challenge()));
    }

    #[test]
    fn verifies_the_longest_verifier() {
        let longest = "~".repeat(128);
        assert!(verifies(&verifier(&longest), &s256(&longest)));
    }

    #[test]
    fn refuses_other_verifiers() {
        for value in [
            &format!("{VERIFIER}~"),
            "eBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk",
            CHALLENGE,
        ] {
            assert!(!verifies(&verifier(value), &challenge()), "{value}");
        }
        assert!(!verifies(
            &verifier(VERIFIER),
            &CodeChallenge::parse(VERIFIER).unwrap()
        ));
    }

    #[test]
    fn ergo_authenticates_with_its_secret_the_way_it_sends_it() {
        let header = basic(&format!("ergo:{SECRET}"));
        assert!(authenticates_introspection(Some(&header), SECRET));
        let lowercase = header.replacen("Basic", "basic", 1);
        assert!(authenticates_introspection(Some(&lowercase), SECRET));
    }

    #[test]
    fn credentials_are_form_decoded() {
        let header = basic("ergo:a%2Bb%3Ac");
        assert!(authenticates_introspection(Some(&header), "a+b:c"));
    }

    #[test]
    fn anything_else_is_refused() {
        for header in [
            None,
            Some(basic(&format!("gamja:{SECRET}"))),
            Some(basic("ergo:wrong")),
            Some(basic("ergo:")),
            Some(basic(&format!("ergo:{SECRET}x"))),
            Some(basic(&format!("ergo{SECRET}"))),
            Some(format!("Bearer {SECRET}")),
            Some("Basic not-base64!".into()),
        ] {
            assert!(
                !authenticates_introspection(header.as_deref(), SECRET),
                "{header:?}"
            );
        }
        assert!(!authenticates_introspection(Some(&basic("ergo:")), ""));
    }
}
