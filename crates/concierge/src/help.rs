//! The Help tab's topics: short answers to what members ask about most.
//! Articles are data one template renders, their lines written in
//! [`crate::markup`] and their numbers taken from what enforces them.

use std::iter;

use crate::{app::App, bot_rules, invite_rules, upload_rules};

/// A page Help links to, by the task it helps with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Topic {
    pub title: &'static str,
    pub href: &'static str,
}

/// The client guides, which have pages of their own.
pub const OWN_APP: Topic = Topic {
    title: "Use your own app",
    href: "/clients",
};

const DEVICES: Topic = Topic {
    title: "Use weblounge on several devices",
    href: "/help/several-devices",
};

const CHANNEL: Topic = Topic {
    title: "Start a channel and keep it",
    href: "/help/start-a-channel",
};

pub const BOT: Topic = Topic {
    title: "Make a bot",
    href: "/help/make-a-bot",
};

const IMAGE: Topic = Topic {
    title: "Share an image",
    href: "/help/share-an-image",
};

/// The bot template's guide, which shows each way to run a bot.
const BOT_TEMPLATE: &str = "https://github.com/luizribeiro/weblounge/tree/main/templates/bot";

pub struct Article {
    pub topic: Topic,
    /// What the reader can do, in a sentence.
    pub lede: &'static str,
    pub sections: Vec<Section>,
    pub keep_in_mind: Vec<String>,
    pub related: &'static [Topic],
}

/// Steps to follow, or points to know, under a heading unless they come
/// first.
pub struct Section {
    pub heading: Option<&'static str>,
    pub numbered: bool,
    pub lines: Vec<String>,
}

impl Section {
    fn steps(heading: Option<&'static str>, lines: impl Into<Vec<String>>) -> Self {
        Self {
            heading,
            numbered: true,
            lines: lines.into(),
        }
    }

    fn points(heading: Option<&'static str>, lines: impl Into<Vec<String>>) -> Self {
        Self {
            heading,
            numbered: false,
            lines: lines.into(),
        }
    }
}

/// What articles say that depends on the reader and on the network's
/// settings.
pub struct Facts<'a> {
    pub nick: &'a str,
    /// How long the server keeps chat history, in words.
    pub history_kept: String,
    /// Where members open the web client.
    pub webchat: String,
    pub uploads: upload_rules::InWords,
}

impl<'a> Facts<'a> {
    pub fn new(app: &App, nick: &'a str) -> Self {
        Self {
            nick,
            history_kept: invite_rules::in_words(app.history_expiry),
            webchat: app.webchat.redirect_uri(),
            uploads: upload_rules::InWords::default(),
        }
    }
}

impl Article {
    pub fn slug(&self) -> &'static str {
        self.topic.href.rsplit('/').next().unwrap_or_default()
    }
}

pub fn articles(facts: &Facts) -> Vec<Article> {
    vec![
        several_devices(facts),
        start_a_channel(),
        make_a_bot(facts),
        share_an_image(facts),
    ]
}

pub fn article(slug: &str, facts: &Facts) -> Option<Article> {
    articles(facts)
        .into_iter()
        .find(|article| article.slug() == slug)
}

/// What the Help index lists, in order.
pub fn index(facts: &Facts) -> Vec<Topic> {
    iter::once(OWN_APP)
        .chain(articles(facts).iter().map(|article| article.topic))
        .collect()
}

fn several_devices(facts: &Facts) -> Article {
    Article {
        topic: DEVICES,
        lede: "You can be on your phone, your laptop and the web client at once, all as the same nick.",
        sections: vec![
            Section::points(
                None,
                [
                    "Every app you sign in to shares your channels and your history.".into(),
                    "You stay in your channels even when every app is closed, and people see you as away.".into(),
                    "When you come back, your app can catch up on what you missed.".into(),
                ],
            ),
            Section::steps(
                Some("Turn this off"),
                [
                    "Go to the [Account](/account#irc-settings) tab.".into(),
                    "Under **IRC while you're away**, untick **Always on** or **Auto-away**.".into(),
                    "Select **Save**.".into(),
                ],
            ),
        ],
        keep_in_mind: vec![
            format!(
                "Messages are deleted from the server after {}.",
                facts.history_kept
            ),
            "Other people's apps may keep their own copies.".into(),
        ],
        related: &[OWN_APP, CHANNEL],
    }
}

fn start_a_channel() -> Article {
    Article {
        topic: CHANNEL,
        lede: "Any member can start a channel, and make it theirs for good.",
        sections: vec![
            Section::steps(
                Some("Start one"),
                [
                    "Run `/join #name`. If nobody's in it, you start it and become its op (@).".into(),
                    "Say what it's about with `/topic` and a line of text. Only ops can change the topic.".into(),
                ],
            ),
            Section::points(
                Some("Keep it yours"),
                [
                    "While you're its op, run `/msg ChanServ REGISTER #name`. That makes you its founder (~).".into(),
                    "A registered channel keeps its topic and modes, and gives you ~ back whenever you rejoin.".into(),
                ],
            ),
            Section::steps(
                Some("Make it private"),
                [
                    "Run `/mode #name +i`, so people can join only when invited.".into(),
                    "Invite someone with `/invite nick #name`.".into(),
                ],
            ),
        ],
        keep_in_mind: vec![
            "To leave a channel out of `/list` too, run `/mode #name +s`.".into(),
            "With always-on, you stay in your channels, and keep your @, while you're away.".into(),
        ],
        related: &[DEVICES, BOT],
    }
}

fn make_a_bot(facts: &Facts) -> Article {
    Article {
        topic: BOT,
        lede: "A bot is a program that chats from an account of its own. You make the account here and run the program wherever you like.",
        sections: vec![
            Section::steps(
                None,
                [
                    "Make its account on the [Bots](/bots) tab, and copy its password. It's shown only once.".into(),
                    "Start from our template: `nix flake init -t github:luizribeiro/weblounge#bot`.".into(),
                    "Set `IRC_NICK` to the bot's name, `IRC_CHANNELS` to the channels it joins and `IRC_PASSWORD_FILE` to a file holding its password.".into(),
                    format!("Run it as a NixOS service, a systemd unit or a container. The [template's guide]({BOT_TEMPLATE}) shows each one."),
                ],
            ),
            Section::points(
                Some("What everyone sees"),
                [
                    format!("Your bot connects as `weblounge/bot/{}`, so everyone can tell it's yours.", facts.nick),
                    "It marks itself as a bot (`+B`).".into(),
                    "It can't join #lounge. Use #bots, or a channel of your own.".into(),
                ],
            ),
        ],
        keep_in_mind: vec![
            format!("You can have up to {} bots once you've confirmed your email.", bot_rules::MAX_LIVE),
            "A bot's name is its nick for good.".into(),
            "Bots aren't always-on: a bot you stop leaves its channels.".into(),
        ],
        related: &[CHANNEL, IMAGE],
    }
}

fn share_an_image(facts: &Facts) -> Article {
    let rules = &facts.uploads;
    Article {
        topic: IMAGE,
        lede: "Paste or attach an image in the chat, and everyone there gets a link to it.",
        sections: vec![
            Section::steps(
                None,
                [
                    format!("Open a channel or a chat in [the web client]({}), or in your IRC app if it can upload.", facts.webchat),
                    "Paste the image, or attach it. Its link goes to the chat.".into(),
                ],
            ),
            Section::points(
                Some("Limits"),
                [
                    format!("Images can be {}, up to {} each and {} an hour.", rules.types, rules.max, rules.per_hour),
                    format!("You have {} in all, and your bots' uploads count toward it.", rules.quota),
                    format!("Each image is deleted after {}. Your [Uploads](/uploads) tab lists yours with the days left, and you can delete one sooner.", rules.kept),
                ],
            ),
        ],
        keep_in_mind: vec![
            "Anyone with an image's link can see it. The link can't be guessed, and we list it only on your Uploads tab.".into(),
            "Where and when a photo was taken, the camera and anything else stored in the file are removed, and so is the file's name.".into(),
            "People who saw an image may keep their own copy.".into(),
        ],
        related: &[BOT, OWN_APP],
    }
}
