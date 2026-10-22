//! Reading a service's configuration from its environment, and its
//! secrets from the credentials systemd hands it.

use std::{fmt, path::Path};

use anyhow::Context;
use secret::SecretString;

#[derive(Debug, PartialEq)]
pub enum ConfigError {
    Missing(&'static str),
    Invalid {
        name: &'static str,
        value: String,
        reason: &'static str,
    },
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::Missing(name) => write!(f, "{name} is not set"),
            Self::Invalid {
                name,
                value,
                reason,
            } => write!(f, "{name}={value:?} is invalid: {reason}"),
        }
    }
}

impl std::error::Error for ConfigError {}

pub fn required(
    var: impl Fn(&str) -> Option<String>,
    name: &'static str,
) -> Result<String, ConfigError> {
    var(name).ok_or(ConfigError::Missing(name))
}

pub fn invalid(name: &'static str, value: String, reason: &'static str) -> ConfigError {
    ConfigError::Invalid {
        name,
        value,
        reason,
    }
}

/// The credential `name` in `credentials_dir`, without the newline a
/// generated one ends in.
pub fn read_credential(credentials_dir: &Path, name: &str) -> anyhow::Result<SecretString> {
    let path = credentials_dir.join(name);
    let mut value =
        std::fs::read_to_string(&path).with_context(|| format!("can't read {}", path.display()))?;
    trim_in_place(&mut value);
    Ok(SecretString::new(value))
}

/// Trims `value` without copying it, so no copy is left unwiped.
fn trim_in_place(value: &mut String) {
    value.truncate(value.trim_end().len());
    let leading = value.len() - value.trim_start().len();
    value.drain(..leading);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_the_variable_and_why_its_value_is_wrong() {
        let var = |name: &str| (name == "CONCIERGE_SMTP_PORT").then(|| "smtps".to_owned());
        assert_eq!(
            required(var, "CONCIERGE_SMTP_HOST")
                .unwrap_err()
                .to_string(),
            "CONCIERGE_SMTP_HOST is not set"
        );
        let port = required(var, "CONCIERGE_SMTP_PORT").unwrap();
        assert_eq!(
            invalid("CONCIERGE_SMTP_PORT", port, "expected a port").to_string(),
            r#"CONCIERGE_SMTP_PORT="smtps" is invalid: expected a port"#
        );
    }

    #[test]
    fn reads_a_credential_trimmed_or_names_the_file_it_cant_read() {
        let dir = tempfile::tempdir().unwrap();
        let error = read_credential(dir.path(), "smtp-password").unwrap_err();
        assert!(error.to_string().ends_with("/smtp-password"), "{error}");
        std::fs::write(dir.path().join("smtp-password"), " hunter2\n").unwrap();
        assert_eq!(
            read_credential(dir.path(), "smtp-password")
                .unwrap()
                .expose(),
            "hunter2"
        );
    }
}
