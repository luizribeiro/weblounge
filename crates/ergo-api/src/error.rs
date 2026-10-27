use reqwest::StatusCode;
use serde::{Deserialize, Serialize};

pub type Result<T, E = Error> = std::result::Result<T, E>;

/// Why a call to Ergo's API failed.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// Ergo didn't accept the bearer token; carries its plain-text answer.
    #[error("Ergo rejected the API token: {0}")]
    Unauthorized(String),
    /// Ergo answered with an HTTP status other than 200 or 401.
    #[error("Ergo answered {status}: {body}")]
    Status { status: StatusCode, body: String },
    /// Ergo understood the request and answered `success: false`.
    #[error("Ergo refused the request: {}", describe_refusal(code.as_ref(), message.as_deref()))]
    Refused {
        code: Option<ErrorCode>,
        message: Option<String>,
    },
    /// Ergo couldn't be reached, or the connection failed mid-request.
    #[error("could not talk to Ergo")]
    Transport(#[from] reqwest::Error),
    /// Ergo's answer wasn't the JSON this crate expects.
    #[error("could not decode Ergo's answer")]
    Decode(#[from] serde_json::Error),
}

impl Error {
    /// The `errorCode` Ergo refused the request with, if any.
    pub fn code(&self) -> Option<&ErrorCode> {
        match self {
            Self::Refused { code, .. } => code.as_ref(),
            _ => None,
        }
    }

    /// Whether the connection failed once made, so Ergo may have carried
    /// the request out though no answer came back.
    pub fn unanswered(&self) -> bool {
        matches!(self, Self::Transport(error) if !error.is_connect())
    }
}

fn describe_refusal(code: Option<&ErrorCode>, message: Option<&str>) -> String {
    match (code, message) {
        (Some(code), Some(message)) => format!("{code} ({message})"),
        (Some(code), None) => code.to_string(),
        (None, Some(message)) => message.to_owned(),
        (None, None) => "no reason given".to_owned(),
    }
}

macro_rules! error_codes {
    ($($variant:ident = $code:literal),* $(,)?) => {
        /// The machine-readable `errorCode` of a refusal.
        #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
        #[serde(from = "String", into = "String")]
        #[non_exhaustive]
        pub enum ErrorCode {
            $(#[doc = concat!("`", $code, "`")] $variant,)*
            /// A code this crate doesn't know yet.
            Other(String),
        }

        impl ErrorCode {
            /// The code as Ergo spells it.
            pub fn as_str(&self) -> &str {
                match self {
                    $(Self::$variant => $code,)*
                    Self::Other(code) => code,
                }
            }
        }

        impl From<String> for ErrorCode {
            fn from(code: String) -> Self {
                match code.as_str() {
                    $($code => Self::$variant,)*
                    _ => Self::Other(code),
                }
            }
        }
    };
}

error_codes! {
    AccountExists = "ACCOUNT_EXISTS",
    AccountDoesNotExist = "ACCOUNT_DOES_NOT_EXIST",
    AccountUnverified = "ACCOUNT_UNVERIFIED",
    InvalidPassphrase = "INVALID_PASSPHRASE",
    InvalidRequest = "INVALID_REQUEST",
    CredentialsExternallyManaged = "CREDENTIALS_EXTERNALLY_MANAGED",
    InvalidVhost = "INVALID_VHOST",
    FeatureDisabled = "FEATURE_DISABLED",
    UnknownError = "UNKNOWN_ERROR",
}

impl From<ErrorCode> for String {
    fn from(code: ErrorCode) -> Self {
        code.as_str().to_owned()
    }
}

impl std::fmt::Display for ErrorCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_round_trip_through_their_spelling() {
        for code in ["ACCOUNT_EXISTS", "INVALID_REQUEST", "UNKNOWN_ERROR"] {
            assert_eq!(ErrorCode::from(code.to_owned()).as_str(), code);
        }
        assert_eq!(
            ErrorCode::from("ACCOUNT_EXISTS".to_owned()),
            ErrorCode::AccountExists
        );
    }

    #[test]
    fn unknown_codes_are_kept_verbatim() {
        let code = ErrorCode::from("RATE_LIMITED".to_owned());
        assert_eq!(code, ErrorCode::Other("RATE_LIMITED".to_owned()));
        assert_eq!(code.to_string(), "RATE_LIMITED");
    }

    #[test]
    fn refusals_describe_whatever_ergo_said() {
        let refused = |code: Option<ErrorCode>, message: Option<&str>| {
            Error::Refused {
                code,
                message: message.map(str::to_owned),
            }
            .to_string()
        };
        assert_eq!(
            refused(
                Some(ErrorCode::AccountExists),
                Some("Name reserved due to a prior registration")
            ),
            "Ergo refused the request: ACCOUNT_EXISTS (Name reserved due to a prior registration)"
        );
        assert_eq!(
            refused(Some(ErrorCode::InvalidRequest), None),
            "Ergo refused the request: INVALID_REQUEST"
        );
        assert_eq!(
            refused(None, Some("database is locked")),
            "Ergo refused the request: database is locked"
        );
        assert_eq!(
            refused(None, None),
            "Ergo refused the request: no reason given"
        );
    }
}
