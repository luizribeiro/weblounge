//! How to log in to IRC with a client certificate, SASL EXTERNAL, instead
//! of a password. Ergo's API can't attach certificates, so members do it
//! themselves through NickServ.

use crate::clients::Snippet;

const CERT: &str = "weblounge.crt";
const KEY: &str = "weblounge.key";

/// The commands and configs the guide walks through, for `nick`.
pub struct Certificates {
    pub generate: Snippet,
    pub fingerprint: Snippet,
    pub register: Snippet,
    pub halloy: Snippet,
    pub weechat_pem: Snippet,
    pub weechat: Snippet,
}

impl Certificates {
    pub fn for_nick(nick: &str) -> Self {
        Self {
            generate: generate(nick),
            fingerprint: snippet(
                "shell",
                format!(
                    "openssl x509 -in {CERT} -noout -fingerprint -sha256 \\\n  | cut -d= -f2 | tr -d : | tr A-F a-f\n"
                ),
            ),
            register: snippet("IRC", "/msg NickServ CERT ADD <fingerprint>\n".into()),
            halloy: snippet(
                "config.toml",
                format!(
                    "# replaces [servers.weblounge.sasl.plain]\n\
                     [servers.weblounge.sasl.external]\n\
                     cert = \"/path/to/{CERT}\"\n\
                     key = \"/path/to/{KEY}\"\n"
                ),
            ),
            weechat_pem: snippet(
                "shell",
                format!(
                    "mkdir -p ~/.config/weechat/tls\n\
                     cat {KEY} {CERT} > ~/.config/weechat/tls/weblounge.pem\n"
                ),
            ),
            weechat: snippet(
                "WeeChat",
                "/set irc.server.weblounge.tls_cert \"${weechat_config_dir}/tls/weblounge.pem\"\n\
                 /set irc.server.weblounge.sasl_mechanism external\n\
                 /reconnect weblounge\n"
                    .into(),
            ),
        }
    }
}

/// A self-signed ECDSA P-256 certificate and its key: Ergo only reads the
/// fingerprint, so nothing needs to sign it.
fn generate(nick: &str) -> Snippet {
    snippet(
        "shell",
        format!(
            "openssl req -x509 -newkey ec -pkeyopt ec_paramgen_curve:prime256v1 \\\n  \
             -nodes -days 3650 -subj /CN={nick} -keyout {KEY} -out {CERT}\n"
        ),
    )
}

fn snippet(target: &'static str, text: String) -> Snippet {
    Snippet { target, text }
}
