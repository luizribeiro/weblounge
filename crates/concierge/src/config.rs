use std::{
    net::SocketAddr,
    path::{Path, PathBuf},
};

use common::config::{invalid, read_credential, required};
use ergo_api::{Client, Url};

pub use crate::routes::presence::Presence;
use crate::{
    history,
    origin::{self, FilesOrigin, Origin, SiteOrigin, WebchatOrigin},
    uploads::Store,
};
pub use common::config::ConfigError;

pub const SESSION_KEY_CREDENTIAL: &str = "session-key";
pub const ERGO_API_TOKEN_CREDENTIAL: &str = "ergo-api-token";
pub const INTROSPECTION_SECRET_CREDENTIAL: &str = "oauth-introspection-secret";
const DEFAULT_ERGO_API: &str = "http://127.0.0.1:8089";
const DATABASE: &str = "concierge.db";
const MIN_SESSION_KEY_BYTES: usize = 32;

#[derive(Debug, Clone, PartialEq)]
pub struct Config {
    pub listen: SocketAddr,
    pub public_url: SiteOrigin,
    /// What invite links start with; the code follows.
    pub invite_links: String,
    /// The web client's origin, the one OAuth client.
    pub webchat_url: WebchatOrigin,
    /// Where uploads are served from.
    pub files_url: FilesOrigin,
    /// Where uploads are handed to the scrubber, and stored.
    pub uploads: Store,
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
    /// How long Ergo keeps chat history, in seconds: its
    /// `history.restrictions.expire-time`.
    pub history_expiry: i64,
}

/// The databases `concierge migrate` brings up to date: concierge's own in
/// `STATE_DIRECTORY`, and the outbox.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Databases {
    pub concierge: PathBuf,
    pub outbox: PathBuf,
}

/// What `concierge remind` needs: the databases, and the site its links
/// lead to. It talks to nothing else, so it reads no credentials.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemindConfig {
    pub databases: Databases,
    pub public_url: SiteOrigin,
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
        let history_expiry = require("CONCIERGE_HISTORY_EXPIRE_TIME")?;
        let public_url = origin("CONCIERGE_PUBLIC_URL", public_url)?;
        let join_url = join_url
            .map(|url| checked_origin("CONCIERGE_JOIN_URL", url, origin::normalize))
            .transpose()?;
        Ok(Self {
            listen: listen.parse().map_err(|_| {
                invalid(
                    "CONCIERGE_LISTEN",
                    listen.clone(),
                    "expected an address:port",
                )
            })?,
            invite_links: invite_links(public_url.as_str(), join_url.as_deref()),
            public_url,
            webchat_url: origin("CONCIERGE_WEBCHAT_URL", webchat_url)?,
            files_url: origin("CONCIERGE_FILES_URL", require("CONCIERGE_FILES_URL")?)?,
            uploads: Store::from_vars(&var)?,
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
            history_expiry: history::parse_expire_time(&history_expiry).ok_or_else(|| {
                invalid(
                    "CONCIERGE_HISTORY_EXPIRE_TIME",
                    history_expiry.clone(),
                    "expected an Ergo duration longer than 0, such as 2w or 14d",
                )
            })?,
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

/// What `concierge expire-uploads` needs: concierge's database and the
/// uploads directory. It talks to nothing else.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExpiryConfig {
    pub database: PathBuf,
    pub uploads: Store,
}

impl ExpiryConfig {
    pub fn from_env() -> Result<Self, ConfigError> {
        Self::from_vars(|name| std::env::var(name).ok())
    }

    pub fn from_vars(var: impl Fn(&str) -> Option<String>) -> Result<Self, ConfigError> {
        Ok(Self {
            database: database_in(Path::new(&required(&var, "STATE_DIRECTORY")?)),
            uploads: Store::from_vars(&var)?,
        })
    }
}

impl RemindConfig {
    pub fn from_env() -> Result<Self, ConfigError> {
        Self::from_vars(|name| std::env::var(name).ok())
    }

    pub fn from_vars(var: impl Fn(&str) -> Option<String>) -> Result<Self, ConfigError> {
        let public_url = required(&var, "CONCIERGE_PUBLIC_URL")?;
        Ok(Self {
            databases: Databases::from_vars(&var)?,
            public_url: origin("CONCIERGE_PUBLIC_URL", public_url)?,
        })
    }
}

fn database_in(state_dir: &Path) -> PathBuf {
    state_dir.join(DATABASE)
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
fn origin<S>(name: &'static str, url: String) -> Result<Origin<S>, ConfigError> {
    checked_origin(name, url, Origin::parse)
}

fn checked_origin<T>(
    name: &'static str,
    url: String,
    parse: impl FnOnce(&str) -> Option<T>,
) -> Result<T, ConfigError> {
    parse(&url).ok_or_else(|| invalid(name, url, "expected an http(s) origin, without a path"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    const PRODUCTION: [(&str, &str); 15] = [
        ("CONCIERGE_LISTEN", "127.0.0.1:8080"),
        ("CONCIERGE_PUBLIC_URL", "https://members.weblounge.org"),
        ("CONCIERGE_JOIN_URL", "https://join.weblounge.org"),
        ("CONCIERGE_WEBCHAT_URL", "https://irc.weblounge.org"),
        ("CONCIERGE_FILES_URL", "https://files.weblounge.org"),
        ("CONCIERGE_UPLOADS", "/var/lib/concierge-files"),
        ("CONCIERGE_SCRUBBER", "/run/concierge-scrubber.sock"),
        ("CONCIERGE_IRC_SERVER", "irc.weblounge.org:6697"),
        ("CONCIERGE_ALWAYS_ON_DEFAULT", "opt-out"),
        ("CONCIERGE_AUTO_AWAY_DEFAULT", "opt-out"),
        ("CONCIERGE_HISTORY_EXPIRE_TIME", "2w"),
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
        parse_with(&[("CONCIERGE_PUBLIC_URL", Some(url))])
            .map(|config| config.public_url.to_string())
    }

    fn origin<S>(url: &str) -> Origin<S> {
        Origin::parse(url).unwrap()
    }

    #[test]
    fn parses_the_production_environment() {
        assert_eq!(
            parse_with(&[]),
            Ok(Config {
                listen: "127.0.0.1:8080".parse().unwrap(),
                public_url: origin("https://members.weblounge.org"),
                invite_links: "https://join.weblounge.org/#".into(),
                webchat_url: origin("https://irc.weblounge.org"),
                files_url: origin("https://files.weblounge.org"),
                uploads: Store {
                    directory: "/var/lib/concierge-files".into(),
                    scrubber: "/run/concierge-scrubber.sock".into(),
                },
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
                history_expiry: 14 * 24 * 60 * 60,
            })
        );
    }

    #[test]
    fn reads_the_history_expiry_in_ergos_format_and_refuses_forever() {
        let expiry = |setting| {
            parse_with(&[("CONCIERGE_HISTORY_EXPIRE_TIME", Some(setting))])
                .map(|config| config.history_expiry)
        };
        assert_eq!(expiry("7d"), Ok(7 * 24 * 60 * 60));
        assert_eq!(expiry("24h"), Ok(24 * 60 * 60));
        let error = expiry("0").unwrap_err();
        assert_eq!(
            error.to_string(),
            r#"CONCIERGE_HISTORY_EXPIRE_TIME="0" is invalid: expected an Ergo duration longer than 0, such as 2w or 14d"#
        );
        assert!(matches!(
            expiry("2 weeks"),
            Err(ConfigError::Invalid { .. })
        ));
        assert_eq!(
            parse_with(&[("CONCIERGE_HISTORY_EXPIRE_TIME", None)]),
            Err(ConfigError::Missing("CONCIERGE_HISTORY_EXPIRE_TIME"))
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
                .webchat_url
                .as_str(),
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

    #[test]
    fn needs_where_uploads_go_and_where_they_are_served() {
        for name in [
            "CONCIERGE_FILES_URL",
            "CONCIERGE_UPLOADS",
            "CONCIERGE_SCRUBBER",
        ] {
            assert_eq!(parse_with(&[(name, None)]), Err(ConfigError::Missing(name)));
        }
        let files = parse_with(&[("CONCIERGE_FILES_URL", Some("https://files.weblounge.org/x"))]);
        assert!(matches!(files, Err(ConfigError::Invalid { .. })));
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

    #[test]
    fn reminding_needs_the_databases_and_the_public_url_alone() {
        let vars = HashMap::from([
            ("STATE_DIRECTORY", "/var/lib/concierge"),
            ("CONCIERGE_OUTBOX", "/var/lib/concierge-outbox/outbox.db"),
            ("CONCIERGE_PUBLIC_URL", "https://members.weblounge.org/"),
        ]);
        let remind = |missing: Option<&str>| {
            RemindConfig::from_vars(|name| {
                (Some(name) != missing)
                    .then(|| vars.get(name).map(|value| (*value).to_owned()))
                    .flatten()
            })
        };
        assert_eq!(
            remind(None),
            Ok(RemindConfig {
                databases: Databases {
                    concierge: "/var/lib/concierge/concierge.db".into(),
                    outbox: "/var/lib/concierge-outbox/outbox.db".into(),
                },
                public_url: origin("https://members.weblounge.org"),
            })
        );
        for name in [
            "STATE_DIRECTORY",
            "CONCIERGE_OUTBOX",
            "CONCIERGE_PUBLIC_URL",
        ] {
            assert_eq!(remind(Some(name)), Err(ConfigError::Missing(name)));
        }
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
}
