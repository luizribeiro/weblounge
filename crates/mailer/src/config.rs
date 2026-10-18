use std::path::PathBuf;

use common::config::{ConfigError, invalid, read_credential, required};
use lettre::message::Mailbox;

pub const SMTP_PASSWORD_CREDENTIAL: &str = "smtp-password";
/// SMTP submission over implicit TLS.
const DEFAULT_SMTP_PORT: u16 = 465;

/// What concierge-mailer needs: the outbox it sends, and the SMTP server it
/// logs in to, over implicit TLS.
#[derive(Debug, Clone, PartialEq)]
pub struct MailerConfig {
    pub smtp_host: String,
    pub smtp_port: u16,
    pub smtp_username: String,
    pub from: Mailbox,
    pub outbox_path: PathBuf,
    pub credentials_dir: PathBuf,
}

impl MailerConfig {
    pub fn from_env() -> Result<Self, ConfigError> {
        Self::from_vars(|name| std::env::var(name).ok())
    }

    pub fn from_vars(var: impl Fn(&str) -> Option<String>) -> Result<Self, ConfigError> {
        let require = |name| required(&var, name);
        let smtp_host = require("CONCIERGE_SMTP_HOST")?;
        let smtp_port = var("CONCIERGE_SMTP_PORT").map_or(Ok(DEFAULT_SMTP_PORT), |port| {
            port.parse()
                .ok()
                .filter(|&port| port != 0)
                .ok_or_else(|| invalid("CONCIERGE_SMTP_PORT", port, "expected a port"))
        })?;
        let from = require("CONCIERGE_MAIL_FROM")?;
        if smtp_host.is_empty() || smtp_host.contains([':', '/']) {
            return Err(invalid(
                "CONCIERGE_SMTP_HOST",
                smtp_host,
                "expected a host name, without a scheme or port",
            ));
        }
        Ok(Self {
            smtp_host,
            smtp_port,
            smtp_username: require("CONCIERGE_SMTP_USERNAME")?,
            from: from.parse().map_err(|_| {
                invalid(
                    "CONCIERGE_MAIL_FROM",
                    from.clone(),
                    "expected an address, or a name and an <address>",
                )
            })?,
            outbox_path: outbox_path(&var)?,
            credentials_dir: require("CREDENTIALS_DIRECTORY")?.into(),
        })
    }

    /// The password of the SMTP server's account.
    pub fn read_smtp_password(&self) -> anyhow::Result<String> {
        let password = read_credential(&self.credentials_dir, SMTP_PASSWORD_CREDENTIAL)?;
        anyhow::ensure!(
            !password.is_empty(),
            "the {SMTP_PASSWORD_CREDENTIAL} credential is empty"
        );
        Ok(password)
    }
}

/// The outbox alone, which is all withdrawing the key takes: systemd
/// gives the commands a unit runs as it stops no credentials.
pub fn outbox_path_from_env() -> Result<PathBuf, ConfigError> {
    outbox_path(|name| std::env::var(name).ok())
}

fn outbox_path(var: impl Fn(&str) -> Option<String>) -> Result<PathBuf, ConfigError> {
    required(var, "CONCIERGE_OUTBOX").map(PathBuf::from)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    const MAILER: [(&str, &str); 5] = [
        ("CONCIERGE_SMTP_HOST", "shadow.mxrouting.net"),
        ("CONCIERGE_SMTP_USERNAME", "noreply@weblounge.org"),
        ("CONCIERGE_MAIL_FROM", "weblounge <noreply@weblounge.org>"),
        ("CONCIERGE_OUTBOX", "/var/lib/concierge-outbox/outbox.db"),
        (
            "CREDENTIALS_DIRECTORY",
            "/run/credentials/concierge-mailer.service",
        ),
    ];

    fn mailer_with(overrides: &[(&str, Option<&str>)]) -> Result<MailerConfig, ConfigError> {
        let mut vars: HashMap<&str, Option<&str>> =
            MAILER.iter().map(|&(k, v)| (k, Some(v))).collect();
        vars.extend(overrides.iter().copied());
        MailerConfig::from_vars(|name| vars.get(name).copied().flatten().map(str::to_owned))
    }

    #[test]
    fn the_mailer_needs_only_smtp_settings_the_outbox_and_its_credentials() {
        let config = mailer_with(&[]).unwrap();
        assert_eq!(
            config,
            MailerConfig {
                smtp_host: "shadow.mxrouting.net".into(),
                smtp_port: 465,
                smtp_username: "noreply@weblounge.org".into(),
                from: "weblounge <noreply@weblounge.org>".parse().unwrap(),
                outbox_path: "/var/lib/concierge-outbox/outbox.db".into(),
                credentials_dir: "/run/credentials/concierge-mailer.service".into(),
            }
        );
        assert_eq!(config.from.email.to_string(), "noreply@weblounge.org");
        assert_eq!(
            mailer_with(&[("CONCIERGE_SMTP_USERNAME", None)]),
            Err(ConfigError::Missing("CONCIERGE_SMTP_USERNAME"))
        );
    }

    #[test]
    fn the_mailer_reaches_a_test_server_on_another_port() {
        let config = mailer_with(&[
            ("CONCIERGE_SMTP_HOST", Some("mail.weblounge.localhost")),
            ("CONCIERGE_SMTP_PORT", Some("1465")),
        ]);
        let config = config.unwrap();
        assert_eq!(
            (config.smtp_host.as_str(), config.smtp_port),
            ("mail.weblounge.localhost", 1465)
        );
    }

    #[test]
    fn rejects_smtp_hosts_ports_and_senders_it_cant_use() {
        for (name, value) in [
            ("CONCIERGE_SMTP_HOST", ""),
            ("CONCIERGE_SMTP_HOST", "smtps://shadow.mxrouting.net"),
            ("CONCIERGE_SMTP_HOST", "shadow.mxrouting.net:465"),
            ("CONCIERGE_SMTP_PORT", "0"),
            ("CONCIERGE_SMTP_PORT", "smtps"),
            ("CONCIERGE_SMTP_PORT", "65536"),
            ("CONCIERGE_MAIL_FROM", "weblounge"),
            ("CONCIERGE_MAIL_FROM", "weblounge <noreply@weblounge.org"),
        ] {
            let error = mailer_with(&[(name, Some(value))]).unwrap_err();
            assert!(
                matches!(error, ConfigError::Invalid { name: invalid, .. } if invalid == name),
                "{name}={value}: {error}"
            );
        }
    }

    #[test]
    fn reads_the_smtp_password_and_refuses_an_empty_one() {
        let dir = tempfile::tempdir().unwrap();
        let config = MailerConfig {
            credentials_dir: dir.path().into(),
            ..mailer_with(&[]).unwrap()
        };
        assert!(config.read_smtp_password().is_err());
        let path = dir.path().join(SMTP_PASSWORD_CREDENTIAL);
        std::fs::write(&path, "\n").unwrap();
        let error = config.read_smtp_password().unwrap_err().to_string();
        assert!(error.contains("is empty"), "{error}");
        std::fs::write(&path, "correct horse battery staple\n").unwrap();
        assert_eq!(
            config.read_smtp_password().unwrap(),
            "correct horse battery staple"
        );
    }
}
