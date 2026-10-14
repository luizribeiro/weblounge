//! The IRC clients we recommend, and how to point each at the network:
//! one table drives the welcome page's picks and every setup guide.

use crate::{config::IrcServer, platform::Platform};

/// What guides put where the nick goes when nobody's signed in.
pub const NICK_PLACEHOLDER: &str = "YOUR_NICK";
const CHANNEL: &str = "#lounge";
/// What clients assume when a server's address names no port.
const DEFAULT_TLS_PORT: u16 = 6697;

pub struct Client {
    pub slug: &'static str,
    pub name: &'static str,
    pub runs_on: &'static str,
    pub price: &'static str,
    pub links: &'static [Link],
    /// How to set it up, in a line or two, next to the connection details.
    pub setup: &'static str,
    config: Option<Format>,
    /// Whether it opens `ircs://` links with the server filled in.
    opens_ircs_links: bool,
    /// Whether it's our web client, which needs no setup at all.
    pub in_browser: bool,
}

pub struct Link {
    pub label: &'static str,
    pub url: &'static str,
}

#[derive(Clone, Copy)]
enum Format {
    HalloyToml,
    SenpaiScfg,
    WeechatCommands,
}

/// A prefilled configuration to copy, password left out.
pub struct Snippet {
    /// Where it goes.
    pub target: &'static str,
    pub text: String,
}

pub const CLIENTS: [Client; 6] = [
    Client {
        slug: "gamja",
        name: "gamja",
        runs_on: "Any browser",
        price: "Nothing to install",
        links: &[Link {
            label: "Source",
            url: "https://codeberg.org/emersion/gamja",
        }],
        setup: "Nothing to set up: it signs you in with your session here and opens #lounge.",
        config: None,
        opens_ircs_links: false,
        in_browser: true,
    },
    Client {
        slug: "halloy",
        name: "Halloy",
        runs_on: "macOS, Windows, Linux",
        price: "Free, open source",
        links: &[Link {
            label: "halloy.chat",
            url: "https://halloy.chat",
        }],
        setup: "Add this to config.toml in Halloy's config directory, then reload the config.",
        config: Some(Format::HalloyToml),
        opens_ircs_links: true,
        in_browser: false,
    },
    Client {
        slug: "goguma",
        name: "Goguma",
        runs_on: "Android, iPhone, iPad",
        price: "Free on F-Droid, about $1 on the App Store",
        links: &[
            Link {
                label: "F-Droid",
                url: "https://f-droid.org/packages/fr.emersion.goguma/",
            },
            Link {
                label: "App Store",
                url: "https://apps.apple.com/app/goguma-irc/id6470394620",
            },
            Link {
                label: "Source",
                url: "https://codeberg.org/emersion/goguma",
            },
        ],
        setup: "Fill in the server, your nick and your password. TLS is on by default; give the server as host:port if the port isn't 6697.",
        config: None,
        opens_ircs_links: false,
        in_browser: false,
    },
    Client {
        slug: "mango",
        name: "Mango",
        runs_on: "iPhone, iPad, macOS",
        price: "Paid, on the App Store",
        links: &[
            Link {
                label: "App Store",
                url: "https://apps.apple.com/app/mango-irc/id431058916",
            },
            Link {
                label: "mangoirc.chat",
                url: "https://mangoirc.chat",
            },
        ],
        setup: "Add a server with the details below: TLS on, SASL PLAIN with your nick and password.",
        config: None,
        opens_ircs_links: false,
        in_browser: false,
    },
    Client {
        slug: "senpai",
        name: "senpai",
        runs_on: "Terminal",
        price: "Free, open source",
        links: &[Link {
            label: "sr.ht/~delthas/senpai",
            url: "https://sr.ht/~delthas/senpai/",
        }],
        setup: "Save this as ~/.config/senpai/senpai.scfg. Add password-cmd (or password) for your password.",
        config: Some(Format::SenpaiScfg),
        opens_ircs_links: false,
        in_browser: false,
    },
    Client {
        slug: "weechat",
        name: "WeeChat",
        runs_on: "Terminal",
        price: "Free, open source",
        links: &[Link {
            label: "weechat.org",
            url: "https://weechat.org",
        }],
        setup: "Run these in WeeChat, typing your password into the /secure line.",
        config: Some(Format::WeechatCommands),
        opens_ircs_links: false,
        in_browser: false,
    },
];

pub fn by_slug(slug: &str) -> Option<&'static Client> {
    CLIENTS.iter().find(|client| client.slug == slug)
}

/// The clients to lead with on `platform`, best first; every client when
/// the platform is unknown.
pub fn recommended(platform: Option<Platform>) -> Vec<&'static Client> {
    let slugs: &[&str] = match platform {
        Some(Platform::MacOs | Platform::Windows | Platform::Linux) => &["halloy"],
        Some(Platform::Ios) => &["goguma", "mango"],
        Some(Platform::Android) => &["goguma"],
        None => return CLIENTS.iter().collect(),
    };
    slugs.iter().filter_map(|slug| by_slug(slug)).collect()
}

impl Client {
    pub fn guide_url(&self) -> String {
        format!("/clients/{}", self.slug)
    }

    /// Its configuration for `nick`, or for a placeholder nick.
    pub fn snippet(&self, server: &IrcServer, nick: Option<&str>) -> Option<Snippet> {
        let nick = nick.unwrap_or(NICK_PLACEHOLDER);
        let IrcServer { host, port } = server;
        let (target, text) = match self.config? {
            Format::HalloyToml => (
                "config.toml",
                format!(
                    "[servers.weblounge]\n\
                     nickname = \"{nick}\"\n\
                     server = \"{host}\"\n\
                     port = {port}\n\
                     use_tls = true\n\
                     \n\
                     [servers.weblounge.sasl.plain]\n\
                     username = \"{nick}\"\n\
                     # password = \"…\", or password_keyring = true\n"
                ),
            ),
            Format::SenpaiScfg => (
                "senpai.scfg",
                format!(
                    "address ircs://{host}:{port}\n\
                     nickname {nick}\n\
                     username {nick}\n\
                     realname {nick}\n\
                     # password-cmd pass show weblounge\n"
                ),
            ),
            Format::WeechatCommands => (
                "WeeChat",
                format!(
                    "/server add weblounge {host}/{port} -tls\n\
                     /set irc.server.weblounge.nicks \"{nick}\"\n\
                     /set irc.server.weblounge.sasl_mechanism plain\n\
                     /set irc.server.weblounge.sasl_username \"{nick}\"\n\
                     /secure set weblounge YOUR_PASSWORD\n\
                     /set irc.server.weblounge.sasl_password \"${{sec.data.weblounge}}\"\n\
                     /set irc.server.weblounge.autoconnect on\n\
                     /connect weblounge\n"
                ),
            ),
        };
        Some(Snippet { target, text })
    }

    /// A link that opens the server in the client, for those that handle
    /// one. It carries no credentials.
    pub fn ircs_link(&self, server: &IrcServer) -> Option<String> {
        self.opens_ircs_links.then(|| {
            let IrcServer { host, port } = server;
            let port = if *port == DEFAULT_TLS_PORT {
                String::new()
            } else {
                format!(":{port}")
            };
            format!("ircs://{host}{port}/{CHANNEL}")
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn production() -> IrcServer {
        IrcServer {
            host: "irc.weblounge.org".into(),
            port: 6697,
        }
    }

    fn vm() -> IrcServer {
        IrcServer {
            host: "irc.weblounge.localhost".into(),
            port: 16697,
        }
    }

    fn client(slug: &str) -> &'static Client {
        by_slug(slug).unwrap()
    }

    fn snippet(slug: &str, server: &IrcServer, nick: Option<&str>) -> String {
        client(slug).snippet(server, nick).unwrap().text
    }

    #[test]
    fn slugs_are_unique_and_fit_in_a_path() {
        for (n, client) in CLIENTS.iter().enumerate() {
            assert!(client.slug.bytes().all(|c| c.is_ascii_lowercase()));
            assert!(CLIENTS[..n].iter().all(|other| other.slug != client.slug));
            assert!(
                client
                    .links
                    .iter()
                    .all(|link| link.url.starts_with("https://"))
            );
        }
        assert!(by_slug("hexchat").is_none());
    }

    #[test]
    fn leads_with_a_client_for_the_platform_and_all_of_them_otherwise() {
        let slugs = |platform| {
            recommended(platform)
                .iter()
                .map(|client| client.slug)
                .collect::<Vec<_>>()
        };
        for platform in [Platform::MacOs, Platform::Windows, Platform::Linux] {
            assert_eq!(slugs(Some(platform)), ["halloy"]);
        }
        assert_eq!(slugs(Some(Platform::Ios)), ["goguma", "mango"]);
        assert_eq!(slugs(Some(Platform::Android)), ["goguma"]);
        assert_eq!(slugs(None).len(), CLIENTS.len());
    }

    #[test]
    fn halloys_config_names_the_server_tls_and_sasl_but_no_password() {
        assert_eq!(
            snippet("halloy", &production(), Some("alice")),
            r#"[servers.weblounge]
nickname = "alice"
server = "irc.weblounge.org"
port = 6697
use_tls = true

[servers.weblounge.sasl.plain]
username = "alice"
# password = "…", or password_keyring = true
"#
        );
    }

    #[test]
    fn senpais_config_uses_an_ircs_address() {
        assert_eq!(
            snippet("senpai", &vm(), Some("alice")),
            "address ircs://irc.weblounge.localhost:16697\n\
             nickname alice\n\
             username alice\n\
             realname alice\n\
             # password-cmd pass show weblounge\n"
        );
    }

    #[test]
    fn weechat_gets_commands_that_keep_the_password_in_secure_data() {
        let commands = snippet("weechat", &production(), Some("alice"));
        assert!(commands.starts_with("/server add weblounge irc.weblounge.org/6697 -tls\n"));
        assert!(commands.contains("/set irc.server.weblounge.sasl_username \"alice\"\n"));
        assert!(commands.contains("sasl_password \"${sec.data.weblounge}\"\n"));
        assert!(commands.ends_with("/connect weblounge\n"));
    }

    #[test]
    fn guests_get_a_placeholder_nick() {
        for slug in ["halloy", "senpai", "weechat"] {
            let text = snippet(slug, &production(), None);
            assert!(text.contains(NICK_PLACEHOLDER), "{slug}: {text}");
        }
    }

    #[test]
    fn gamja_is_the_one_client_that_runs_in_the_browser() {
        let in_browser: Vec<_> = CLIENTS
            .iter()
            .filter(|client| client.in_browser)
            .map(|client| client.slug)
            .collect();
        assert_eq!(in_browser, ["gamja"]);
        assert!(
            client("gamja")
                .snippet(&production(), Some("alice"))
                .is_none()
        );
        assert_eq!(client("gamja").ircs_link(&production()), None);
    }

    #[test]
    fn mobile_clients_are_set_up_by_hand() {
        for slug in ["goguma", "mango"] {
            assert!(client(slug).snippet(&production(), Some("alice")).is_none());
        }
    }

    #[test]
    fn only_halloy_gets_an_ircs_link_with_no_default_port() {
        assert_eq!(
            client("halloy").ircs_link(&production()).as_deref(),
            Some("ircs://irc.weblounge.org/#lounge")
        );
        assert_eq!(
            client("halloy").ircs_link(&vm()).as_deref(),
            Some("ircs://irc.weblounge.localhost:16697/#lounge")
        );
        for slug in ["gamja", "goguma", "mango", "senpai", "weechat"] {
            assert_eq!(client(slug).ircs_link(&production()), None, "{slug}");
        }
    }
}
