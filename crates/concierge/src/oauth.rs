//! Concierge as a small OAuth 2.0 authorization server: the web client
//! signs members in with an authorization code and PKCE, and Ergo checks
//! the tokens it hands over with introspection.
//!
//! Times are Unix seconds.

use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;

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
    pub code_challenge: String,
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
            .filter(|state| (1..=MAX_STATE_LENGTH).contains(&state.len()));
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
            .filter(|challenge| is_challenge(challenge))
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

fn is_challenge(challenge: &str) -> bool {
    challenge.len() == CHALLENGE_LENGTH
        && challenge
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
}

/// Whether `verifier` is the one `challenge` was made from, by S256.
pub fn verifies(verifier: &str, challenge: &str) -> bool {
    let well_formed = VERIFIER_LENGTHS.contains(&verifier.len())
        && verifier
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"-._~".contains(&c));
    let derived = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
    well_formed && bool::from(derived.as_bytes().ct_eq(challenge.as_bytes()))
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
                code_challenge: CHALLENGE.into(),
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

    #[test]
    fn verifies_the_rfc_example() {
        assert!(verifies(VERIFIER, CHALLENGE));
    }

    #[test]
    fn refuses_other_verifiers() {
        let long = "a".repeat(129);
        for verifier in [
            "",
            &VERIFIER[1..],
            "eBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk",
            CHALLENGE,
            &long,
        ] {
            assert!(!verifies(verifier, CHALLENGE), "{verifier}");
        }
        assert!(!verifies(VERIFIER, VERIFIER));
    }

    #[test]
    fn refuses_verifiers_outside_rfc_7636s_alphabet_and_lengths() {
        for verifier in ["a".repeat(42), format!("{} ", "a".repeat(43))] {
            let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
            assert!(!verifies(&verifier, &challenge), "{verifier:?}");
        }
        let longest = "~".repeat(128);
        let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(longest.as_bytes()));
        assert!(verifies(&longest, &challenge));
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
