use std::{
    fmt,
    net::SocketAddr,
    path::{Path, PathBuf},
};

use anyhow::Context;
use axum::http::Uri;
use ergo_api::{Client, Url};
use lettre::message::Mailbox;

pub use crate::routes::presence::Presence;

pub const SESSION_KEY_CREDENTIAL: &str = "session-key";
pub const ERGO_API_TOKEN_CREDENTIAL: &str = "ergo-api-token";
pub const INTROSPECTION_SECRET_CREDENTIAL: &str = "oauth-introspection-secret";
pub const SMTP_PASSWORD_CREDENTIAL: &str = "smtp-password";
const DEFAULT_ERGO_API: &str = "http://127.0.0.1:8089";
/// SMTP submission over implicit TLS.
const DEFAULT_SMTP_PORT: u16 = 465;
const DATABASE: &str = "concierge.db";
const MIN_SESSION_KEY_BYTES: usize = 32;

#[derive(Debug, Clone, PartialEq)]
pub struct Config {
    pub listen: SocketAddr,
    pub public_url: String,
    /// What invite links start with; the code follows.
    pub invite_links: String,
    /// The web client's origin, the one OAuth client.
    pub webchat_url: String,
    pub irc_server: IrcServer,
    pub ergo_api: Url,
    pub assets_dir: PathBuf,
    pub state_dir: PathBuf,
    /// The outbox's database, which the mailer sends from.
    pub outbox_path: PathBuf,
    pub credentials_dir: PathBuf,
    /// Ergo's multiclient always-on and auto-away, as what members get
    /// until they choose.
    pub presence_defaults: Presence,
}

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

/// The databases `concierge migrate` brings up to date: concierge's own in
/// `STATE_DIRECTORY`, and the outbox.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Databases {
    pub concierge: PathBuf,
    pub outbox: PathBuf,
}

/// The address members' IRC clients connect to, over TLS.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IrcServer {
    pub host: String,
    pub port: u16,
}

impl IrcServer {
    fn parse(address: &str) -> Option<Self> {
        let (host, port) = address.rsplit_once(':')?;
        let port = port.parse().ok().filter(|&port| port != 0)?;
        (!host.is_empty()).then(|| Self {
            host: host.to_owned(),
            port,
        })
    }
}

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

impl Config {
    pub fn from_env() -> Result<Self, ConfigError> {
        Self::from_vars(|name| std::env::var(name).ok())
    }

    /// `STATE_DIRECTORY` and `CREDENTIALS_DIRECTORY` are the variables
    /// systemd sets for `StateDirectory=` and `LoadCredential=`.
    pub fn from_vars(var: impl Fn(&str) -> Option<String>) -> Result<Self, ConfigError> {
        let require = |name| required(&var, name);

        let listen = require("CONCIERGE_LISTEN")?;
        let public_url = require("CONCIERGE_PUBLIC_URL")?;
        let webchat_url = require("CONCIERGE_WEBCHAT_URL")?;
        let join_url = var("CONCIERGE_JOIN_URL");
        let irc_server = require("CONCIERGE_IRC_SERVER")?;
        let ergo_api = var("CONCIERGE_ERGO_API").unwrap_or_else(|| DEFAULT_ERGO_API.into());
        let multiclient_default = |name| {
            let value = require(name)?;
            parse_multiclient_default(&value).ok_or_else(|| {
                invalid(
                    name,
                    value,
                    "expected opt-in or opt-out; members can't choose under mandatory or disabled",
                )
            })
        };
        let public_url = origin("CONCIERGE_PUBLIC_URL", public_url)?;
        let join_url = join_url
            .map(|url| origin("CONCIERGE_JOIN_URL", url))
            .transpose()?;
        Ok(Self {
            listen: listen.parse().map_err(|_| {
                invalid(
                    "CONCIERGE_LISTEN",
                    listen.clone(),
                    "expected an address:port",
                )
            })?,
            invite_links: invite_links(&public_url, join_url.as_deref()),
            public_url,
            webchat_url: origin("CONCIERGE_WEBCHAT_URL", webchat_url)?,
            irc_server: IrcServer::parse(&irc_server).ok_or_else(|| {
                invalid(
                    "CONCIERGE_IRC_SERVER",
                    irc_server.clone(),
                    "expected a host:port",
                )
            })?,
            ergo_api: parse_http_url(&ergo_api).ok_or_else(|| {
                invalid(
                    "CONCIERGE_ERGO_API",
                    ergo_api.clone(),
                    "expected a plain http URL",
                )
            })?,
            assets_dir: require("CONCIERGE_ASSETS")?.into(),
            state_dir: require("STATE_DIRECTORY")?.into(),
            outbox_path: require("CONCIERGE_OUTBOX")?.into(),
            credentials_dir: require("CREDENTIALS_DIRECTORY")?.into(),
            presence_defaults: Presence {
                always_on: multiclient_default("CONCIERGE_ALWAYS_ON_DEFAULT")?,
                auto_away: multiclient_default("CONCIERGE_AUTO_AWAY_DEFAULT")?,
            },
        })
    }

    pub fn invite_url(&self, code: &str) -> String {
        invite_url(&self.invite_links, code)
    }

    pub fn database_path(&self) -> PathBuf {
        database_in(&self.state_dir)
    }

    pub fn read_session_key(&self) -> anyhow::Result<Vec<u8>> {
        let key = self.read_credential(SESSION_KEY_CREDENTIAL)?;
        anyhow::ensure!(
            key.len() >= MIN_SESSION_KEY_BYTES,
            "the {SESSION_KEY_CREDENTIAL} credential holds {} bytes; a session key needs at least {MIN_SESSION_KEY_BYTES}",
            key.len()
        );
        Ok(key.into_bytes())
    }

    /// A client for Ergo's API, with the token from the credentials.
    pub fn ergo_client(&self) -> anyhow::Result<Client> {
        let token = self.read_credential(ERGO_API_TOKEN_CREDENTIAL)?;
        anyhow::ensure!(
            !token.is_empty(),
            "the {ERGO_API_TOKEN_CREDENTIAL} credential is empty"
        );
        Ok(Client::new(self.ergo_api.clone(), token))
    }

    /// The secret Ergo authenticates with to introspect tokens.
    pub fn read_introspection_secret(&self) -> anyhow::Result<String> {
        let secret = self.read_credential(INTROSPECTION_SECRET_CREDENTIAL)?;
        anyhow::ensure!(
            secret.len() >= MIN_SESSION_KEY_BYTES,
            "the {INTROSPECTION_SECRET_CREDENTIAL} credential holds {} bytes; it needs at least {MIN_SESSION_KEY_BYTES}",
            secret.len()
        );
        Ok(secret)
    }

    fn read_credential(&self, name: &str) -> anyhow::Result<String> {
        read_credential(&self.credentials_dir, name)
    }
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
            outbox_path: require("CONCIERGE_OUTBOX")?.into(),
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

impl Databases {
    pub fn from_env() -> Result<Self, ConfigError> {
        Self::from_vars(|name| std::env::var(name).ok())
    }

    pub fn from_vars(var: impl Fn(&str) -> Option<String>) -> Result<Self, ConfigError> {
        let state_dir = required(&var, "STATE_DIRECTORY")?;
        Ok(Self {
            concierge: database_in(Path::new(&state_dir)),
            outbox: required(&var, "CONCIERGE_OUTBOX")?.into(),
        })
    }
}

fn required(
    var: impl Fn(&str) -> Option<String>,
    name: &'static str,
) -> Result<String, ConfigError> {
    var(name).ok_or(ConfigError::Missing(name))
}

fn invalid(name: &'static str, value: String, reason: &'static str) -> ConfigError {
    ConfigError::Invalid {
        name,
        value,
        reason,
    }
}

fn database_in(state_dir: &Path) -> PathBuf {
    state_dir.join(DATABASE)
}

fn read_credential(credentials_dir: &Path, name: &str) -> anyhow::Result<String> {
    let path = credentials_dir.join(name);
    let value =
        std::fs::read_to_string(&path).with_context(|| format!("can't read {}", path.display()))?;
    Ok(value.trim().to_owned())
}

pub fn invite_url(invite_links: &str, code: &str) -> String {
    format!("{invite_links}{code}")
}

/// Where invite links lead: the join origin, which sends visitors on to
/// the invite page with the fragment, or else the invite page itself.
fn invite_links(public_url: &str, join_url: Option<&str>) -> String {
    match join_url {
        Some(join_url) => format!("{join_url}/#"),
        None => format!("{public_url}/invite#"),
    }
}

/// Whether an Ergo multiclient setting, such as `always-on`, turns the
/// feature on for members who haven't chosen.
fn parse_multiclient_default(setting: &str) -> Option<bool> {
    match setting {
        "opt-out" => Some(true),
        "opt-in" => Some(false),
        _ => None,
    }
}

fn parse_http_url(url: &str) -> Option<Url> {
    Url::parse(url).ok().filter(|url| url.scheme() == "http")
}

/// The origin in the variable `name`, which holds `url`.
fn origin(name: &'static str, url: String) -> Result<String, ConfigError> {
    parse_origin(&url)
        .ok_or_else(|| invalid(name, url, "expected an http(s) origin, without a path"))
}

fn parse_origin(url: &str) -> Option<String> {
    let uri: Uri = url.parse().ok()?;
    let has_path = !matches!(uri.path(), "" | "/") || uri.query().is_some();
    let web_scheme = matches!(uri.scheme_str(), Some("http" | "https"));
    (web_scheme && uri.authority().is_some() && !has_path)
        .then(|| url.trim_end_matches('/').to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    const PRODUCTION: [(&str, &str); 11] = [
        ("CONCIERGE_LISTEN", "127.0.0.1:8080"),
        ("CONCIERGE_PUBLIC_URL", "https://members.weblounge.org"),
        ("CONCIERGE_JOIN_URL", "https://join.weblounge.org"),
        ("CONCIERGE_WEBCHAT_URL", "https://irc.weblounge.org"),
        ("CONCIERGE_IRC_SERVER", "irc.weblounge.org:6697"),
        ("CONCIERGE_ALWAYS_ON_DEFAULT", "opt-out"),
        ("CONCIERGE_AUTO_AWAY_DEFAULT", "opt-out"),
        (
            "CONCIERGE_ASSETS",
            "/nix/store/asqgxh40xn390yzqhq97lkbqf5sslbyr-weblounge-web/members",
        ),
        ("STATE_DIRECTORY", "/var/lib/concierge"),
        ("CONCIERGE_OUTBOX", "/var/lib/concierge-outbox/outbox.db"),
        (
            "CREDENTIALS_DIRECTORY",
            "/run/credentials/concierge.service",
        ),
    ];

    fn parse_with(overrides: &[(&str, Option<&str>)]) -> Result<Config, ConfigError> {
        let mut vars: HashMap<&str, Option<&str>> =
            PRODUCTION.iter().map(|&(k, v)| (k, Some(v))).collect();
        vars.extend(overrides.iter().copied());
        Config::from_vars(|name| vars.get(name).copied().flatten().map(str::to_owned))
    }

    fn ergo_api(url: &str) -> Result<Url, ConfigError> {
        parse_with(&[("CONCIERGE_ERGO_API", Some(url))]).map(|config| config.ergo_api)
    }

    fn public_url(url: &str) -> Result<String, ConfigError> {
        parse_with(&[("CONCIERGE_PUBLIC_URL", Some(url))]).map(|config| config.public_url)
    }

    #[test]
    fn parses_the_production_environment() {
        assert_eq!(
            parse_with(&[]),
            Ok(Config {
                listen: "127.0.0.1:8080".parse().unwrap(),
                public_url: "https://members.weblounge.org".into(),
                invite_links: "https://join.weblounge.org/#".into(),
                webchat_url: "https://irc.weblounge.org".into(),
                irc_server: IrcServer {
                    host: "irc.weblounge.org".into(),
                    port: 6697,
                },
                ergo_api: "http://127.0.0.1:8089".parse().unwrap(),
                assets_dir: "/nix/store/asqgxh40xn390yzqhq97lkbqf5sslbyr-weblounge-web/members"
                    .into(),
                state_dir: "/var/lib/concierge".into(),
                outbox_path: "/var/lib/concierge-outbox/outbox.db".into(),
                credentials_dir: "/run/credentials/concierge.service".into(),
                presence_defaults: Presence {
                    always_on: true,
                    auto_away: true,
                },
            })
        );
    }

    #[test]
    fn opt_in_means_off_until_a_member_chooses() {
        let config = parse_with(&[("CONCIERGE_AUTO_AWAY_DEFAULT", Some("opt-in"))]);
        assert_eq!(
            config.map(|config| config.presence_defaults),
            Ok(Presence {
                always_on: true,
                auto_away: false,
            })
        );
    }

    #[test]
    fn refuses_multiclient_settings_members_cannot_choose_under() {
        for setting in ["mandatory", "disabled", "true", "Opt-Out", ""] {
            let error = parse_with(&[("CONCIERGE_ALWAYS_ON_DEFAULT", Some(setting))]).unwrap_err();
            assert_eq!(
                error.to_string(),
                format!(
                    "CONCIERGE_ALWAYS_ON_DEFAULT={setting:?} is invalid: expected opt-in or opt-out; members can't choose under mandatory or disabled"
                )
            );
        }
        assert_eq!(
            parse_with(&[("CONCIERGE_AUTO_AWAY_DEFAULT", None)]),
            Err(ConfigError::Missing("CONCIERGE_AUTO_AWAY_DEFAULT"))
        );
    }

    #[test]
    fn keeps_a_forwarded_port_and_drops_a_trailing_slash() {
        assert_eq!(
            public_url("https://members.weblounge.localhost:10443/"),
            Ok("https://members.weblounge.localhost:10443".into())
        );
    }

    #[test]
    fn builds_invite_links_on_the_join_url() {
        let config = parse_with(&[]).unwrap();
        assert_eq!(
            config.invite_url("q7Hx2mZp9vLcT4wRfBa-_w"),
            "https://join.weblounge.org/#q7Hx2mZp9vLcT4wRfBa-_w"
        );
    }

    #[test]
    fn without_a_join_url_invite_links_lead_to_the_invite_page() {
        let config = parse_with(&[("CONCIERGE_JOIN_URL", None)]).unwrap();
        assert_eq!(
            config.invite_url("q7Hx2mZp9vLcT4wRfBa-_w"),
            "https://members.weblounge.org/invite#q7Hx2mZp9vLcT4wRfBa-_w"
        );
    }

    #[test]
    fn the_join_url_is_an_origin_too() {
        let join = |url| parse_with(&[("CONCIERGE_JOIN_URL", Some(url))]);
        assert_eq!(
            join("https://join.weblounge.localhost:10443/")
                .unwrap()
                .invite_links,
            "https://join.weblounge.localhost:10443/#"
        );
        assert!(matches!(
            join("https://join.weblounge.org/invite"),
            Err(ConfigError::Invalid { .. })
        ));
    }

    #[test]
    fn rejects_public_urls_that_are_not_web_origins() {
        for url in [
            "members.weblounge.org",
            "ftp://members.weblounge.org",
            "https://weblounge.org/members",
            "https://members.weblounge.org/?invite=1",
        ] {
            assert!(
                matches!(public_url(url), Err(ConfigError::Invalid { .. })),
                "{url}"
            );
        }
    }

    #[test]
    fn the_webchat_url_is_an_origin_too() {
        let webchat = |url| parse_with(&[("CONCIERGE_WEBCHAT_URL", Some(url))]);
        assert_eq!(
            webchat("https://irc.weblounge.localhost:10443/")
                .unwrap()
                .webchat_url,
            "https://irc.weblounge.localhost:10443"
        );
        assert!(matches!(
            webchat("https://irc.weblounge.org/chat"),
            Err(ConfigError::Invalid { .. })
        ));
        assert_eq!(
            parse_with(&[("CONCIERGE_WEBCHAT_URL", None)]),
            Err(ConfigError::Missing("CONCIERGE_WEBCHAT_URL"))
        );
    }

    #[test]
    fn rejects_an_irc_server_without_a_host_or_port() {
        for address in [
            "irc.weblounge.org",
            "irc.weblounge.org:",
            ":6697",
            "irc.weblounge.org:0",
            "irc.weblounge.org:tls",
        ] {
            let config = parse_with(&[("CONCIERGE_IRC_SERVER", Some(address))]);
            assert!(
                matches!(config, Err(ConfigError::Invalid { .. })),
                "{address}"
            );
        }
    }

    #[test]
    fn reaches_ergo_wherever_its_api_listens() {
        assert_eq!(
            ergo_api("http://127.0.0.1:18089"),
            Ok("http://127.0.0.1:18089".parse().unwrap())
        );
    }

    #[test]
    fn rejects_ergo_api_urls_that_are_not_plain_http() {
        for url in ["127.0.0.1:8089", "https://127.0.0.1:8089"] {
            assert!(
                matches!(ergo_api(url), Err(ConfigError::Invalid { .. })),
                "{url}"
            );
        }
    }

    #[test]
    fn rejects_a_listen_address_without_a_port() {
        let error = parse_with(&[("CONCIERGE_LISTEN", Some("127.0.0.1"))]).unwrap_err();
        assert_eq!(
            error.to_string(),
            r#"CONCIERGE_LISTEN="127.0.0.1" is invalid: expected an address:port"#
        );
    }

    #[test]
    fn names_a_missing_variable() {
        let error = parse_with(&[("CREDENTIALS_DIRECTORY", None)]).unwrap_err();
        assert_eq!(error, ConfigError::Missing("CREDENTIALS_DIRECTORY"));
    }

    fn config_with_credentials(dir: &std::path::Path) -> Config {
        Config {
            credentials_dir: dir.into(),
            ..parse_with(&[]).unwrap()
        }
    }

    #[test]
    fn reads_a_generated_session_key_without_its_newline() {
        let dir = tempfile::tempdir().unwrap();
        let generated = "9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08\n";
        std::fs::write(dir.path().join(SESSION_KEY_CREDENTIAL), generated).unwrap();
        let key = config_with_credentials(dir.path())
            .read_session_key()
            .unwrap();
        assert_eq!(key, generated.trim().as_bytes());
    }

    #[test]
    fn refuses_a_short_or_missing_session_key() {
        let dir = tempfile::tempdir().unwrap();
        let config = config_with_credentials(dir.path());
        assert!(config.read_session_key().is_err());
        std::fs::write(dir.path().join(SESSION_KEY_CREDENTIAL), "hunter2\n").unwrap();
        let error = config.read_session_key().unwrap_err().to_string();
        assert!(error.contains("holds 7 bytes"), "{error}");
    }

    #[test]
    fn reads_the_introspection_secret_and_refuses_a_short_one() {
        let dir = tempfile::tempdir().unwrap();
        let config = config_with_credentials(dir.path());
        assert!(config.read_introspection_secret().is_err());
        let path = dir.path().join(INTROSPECTION_SECRET_CREDENTIAL);
        std::fs::write(&path, "hunter2\n").unwrap();
        assert!(config.read_introspection_secret().is_err());
        let generated = "9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08";
        std::fs::write(&path, format!("{generated}\n")).unwrap();
        assert_eq!(config.read_introspection_secret().unwrap(), generated);
    }

    #[test]
    fn builds_an_ergo_client_from_the_generated_token() {
        let dir = tempfile::tempdir().unwrap();
        let config = config_with_credentials(dir.path());
        assert!(config.ergo_client().is_err());
        std::fs::write(dir.path().join(ERGO_API_TOKEN_CREDENTIAL), "\n").unwrap();
        let error = config.ergo_client().unwrap_err().to_string();
        assert!(error.contains("is empty"), "{error}");
        std::fs::write(dir.path().join(ERGO_API_TOKEN_CREDENTIAL), "5e5a1c\n").unwrap();
        let client = config.ergo_client().unwrap();
        assert!(format!("{client:?}").contains("http://127.0.0.1:8089/"));
    }

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
    fn migrating_needs_the_state_directory_and_the_outbox() {
        let vars = HashMap::from([
            ("STATE_DIRECTORY", "/var/lib/concierge"),
            ("CONCIERGE_OUTBOX", "/var/lib/concierge-outbox/outbox.db"),
        ]);
        let databases = |missing: Option<&str>| {
            Databases::from_vars(|name| {
                (Some(name) != missing)
                    .then(|| vars.get(name).map(|value| (*value).to_owned()))
                    .flatten()
            })
        };
        assert_eq!(
            databases(None),
            Ok(Databases {
                concierge: "/var/lib/concierge/concierge.db".into(),
                outbox: "/var/lib/concierge-outbox/outbox.db".into(),
            })
        );
        for name in ["STATE_DIRECTORY", "CONCIERGE_OUTBOX"] {
            assert_eq!(databases(Some(name)), Err(ConfigError::Missing(name)));
        }
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
