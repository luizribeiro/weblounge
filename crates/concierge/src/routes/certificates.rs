//! How to log in to IRC with a client certificate, SASL EXTERNAL, instead
//! of a password, from the apps that can. Ergo's API can't attach
//! certificates, so members do it themselves through NickServ.

use askama::Template;
use axum::{
    extract::{Path, State},
    response::Response,
};

use super::{Layout, not_found, render};
use crate::{
    app::AppState,
    auth::CurrentMember,
    clients::{self, Client, External, Snippet},
    config::IrcServer,
};

const CERT: &str = "weblounge.crt";
const KEY: &str = "weblounge.key";

/// The commands and configs the guide walks through, for `nick` and one app.
pub struct Certificates {
    pub generate: Snippet,
    pub fingerprint: Snippet,
    pub register: Snippet,
    /// What switching the app to SASL EXTERNAL takes, in a line, and the
    /// snippets that do it.
    pub switch_intro: &'static str,
    pub switch_snippets: Vec<Snippet>,
}

impl Certificates {
    pub fn for_app(nick: &str, external: External) -> Self {
        let (switch_intro, switch_snippets) = switch_to_external(external);
        Self {
            generate: generate(nick),
            fingerprint: snippet(
                "shell",
                format!(
                    "openssl x509 -in {CERT} -noout -fingerprint -sha256 \\\n  | cut -d= -f2 | tr -d : | tr A-F a-f\n"
                ),
            ),
            register: snippet("IRC", "/msg NickServ CERT ADD <fingerprint>\n".into()),
            switch_intro,
            switch_snippets,
        }
    }
}

#[derive(Template)]
#[template(path = "certificate.html")]
struct Guide<'a> {
    layout: Layout<'a>,
    irc: &'a IrcServer,
    client: &'a Client,
    certificates: Certificates,
}

pub async fn show(
    State(app): State<AppState>,
    CurrentMember(me): CurrentMember,
    Path(slug): Path<String>,
) -> Response {
    let Some(client) = clients::by_slug(&slug) else {
        return not_found();
    };
    let Some(external) = client.external else {
        return not_found();
    };
    render(&Guide {
        layout: Layout::member(&app, &me.member),
        irc: &app.irc_server,
        client,
        certificates: Certificates::for_app(me.member.account.as_str(), external),
    })
}

fn switch_to_external(external: External) -> (&'static str, Vec<Snippet>) {
    match external {
        External::HalloyFiles => (
            "Swap the sasl.plain section in config.toml for this one, with the paths to your two files:",
            vec![snippet(
                "config.toml",
                format!(
                    "# replaces [servers.weblounge.sasl.plain]\n\
                     [servers.weblounge.sasl.external]\n\
                     cert = \"/path/to/{CERT}\"\n\
                     key = \"/path/to/{KEY}\"\n"
                ),
            )],
        ),
        External::WeechatPem => (
            "WeeChat wants the key and the certificate in one file:",
            vec![
                snippet(
                    "shell",
                    format!(
                        "mkdir -p ~/.config/weechat/tls\n\
                         cat {KEY} {CERT} > ~/.config/weechat/tls/weblounge.pem\n"
                    ),
                ),
                snippet(
                    "WeeChat",
                    "/set irc.server.weblounge.tls_cert \"${weechat_config_dir}/tls/weblounge.pem\"\n\
                     /set irc.server.weblounge.sasl_mechanism external\n\
                     /reconnect weblounge\n"
                        .into(),
                ),
            ],
        ),
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
