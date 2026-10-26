//! The greeter's connection to Ergo, as the house account: logged in with
//! SASL, in bot mode and in #lounge, where it welcomes newcomers.

use anyhow::Context;
use concierge::{config::IrcServer, house};
use papo::{Client, Event, Events, Sasl};
use secret::SecretString;

pub const LOUNGE: &str = "#lounge";

/// How the greeter connects. papo reconnects by itself, until a refusal
/// it takes for permanent, such as a wrong password.
pub fn builder(server: &IrcServer, password: &SecretString) -> papo::Builder {
    Client::builder(format!("ircs://{}:{}", server.host, server.port))
        .nick(house::ACCOUNT)
        .realname("weblounge's host")
        .sasl(Sasl::plain(house::ACCOUNT, password.expose()))
        .bot(true)
        .autojoin([LOUNGE])
}

pub async fn connect(
    server: &IrcServer,
    password: &SecretString,
) -> anyhow::Result<(Client, Events)> {
    builder(server, password)
        .connect()
        .await
        .with_context(|| format!("can't connect to {}:{}", server.host, server.port))
}

/// Logs the connection coming and going, and answers once papo stopped
/// reconnecting, which systemd takes from there.
pub async fn watch(mut events: Events) -> anyhow::Error {
    while let Some(event) = events.next().await {
        match event {
            Event::Connected(_) => eprintln!("Connected to Ergo"),
            Event::Disconnected(disconnected) => eprintln!("Disconnected: {disconnected}"),
            _ => {}
        }
    }
    anyhow::anyhow!("papo stopped reconnecting to Ergo")
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use base64::{Engine, engine::general_purpose::STANDARD};
    use papo_testing::{Wire, wire};

    const PASSWORD: &str = "0f3a9c51d2e84b7f6a0c19e2d4b5a7c8";

    /// The greeter, connected to an in-memory Ergo that logged it in, and
    /// what it sent once in, up to its JOIN.
    pub async fn connected() -> (Client, Events, Wire, Vec<String>) {
        let server = IrcServer {
            host: "irc.weblounge.org".into(),
            port: 6697,
        };
        let builder = builder(&server, &SecretString::from(PASSWORD))
            .rate_limit(papo::RateLimit::unlimited())
            .reconnect(papo::Reconnect::never());
        let (client, events, mut wire) = wire(builder);
        let sent = log_in(&mut wire).await;
        (client, events, wire, sent)
    }

    /// A line the client sent, without the tags papo put on it.
    pub fn untagged(line: &str) -> &str {
        match line.strip_prefix('@') {
            Some(tagged) => tagged.split_once(' ').map_or("", |(_, rest)| rest),
            None => line,
        }
    }

    /// Plays Ergo's side of a SASL login.
    async fn log_in(wire: &mut Wire) -> Vec<String> {
        assert_eq!(wire.next_sent().await, "CAP LS 302");
        assert_eq!(wire.next_sent().await, "NICK concierge");
        assert!(wire.next_sent().await.starts_with("USER "));
        wire.server(
            ":irc.test CAP * LS :sasl=PLAIN echo-message labeled-response message-tags batch",
        )
        .await;
        let requested = wire.next_sent().await;
        let requested = requested.strip_prefix("CAP REQ :").expect("a CAP REQ");
        assert!(requested.split(' ').any(|cap| cap == "sasl"), "{requested}");
        wire.server(format!(":irc.test CAP * ACK :{requested}"))
            .await;
        assert_eq!(wire.next_sent().await, "AUTHENTICATE PLAIN");
        wire.server("AUTHENTICATE +").await;
        let credentials = wire.next_sent().await;
        let credentials = STANDARD
            .decode(credentials.strip_prefix("AUTHENTICATE ").unwrap())
            .unwrap();
        assert_eq!(credentials, format!("\0concierge\0{PASSWORD}").as_bytes());
        wire.server(":irc.test 903 concierge :SASL authentication successful")
            .await;
        assert_eq!(wire.next_sent().await, "CAP END");
        for line in [
            ":irc.test 001 concierge :Welcome to the weblounge IRC Network",
            ":irc.test 005 concierge BOT=B CASEMAPPING=ascii CHANTYPES=# PREFIX=(qaohv)~&@%+ :are supported",
            ":irc.test 422 concierge :MOTD File is missing",
        ] {
            wire.server(line).await;
        }
        let mut sent: Vec<String> = Vec::new();
        while !sent
            .last()
            .is_some_and(|line| untagged(line).starts_with("JOIN "))
        {
            sent.push(wire.next_sent().await);
        }
        sent
    }

    #[tokio::test(start_paused = true)]
    async fn logs_in_as_the_house_in_bot_mode_and_joins_the_lounge() {
        let (_client, _events, _wire, sent) = connected().await;
        assert!(sent.contains(&"MODE concierge +B".to_owned()), "{sent:?}");
        assert_eq!(untagged(sent.last().unwrap()), "JOIN #lounge");
    }
}
