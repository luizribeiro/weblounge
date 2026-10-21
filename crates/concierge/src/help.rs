//! The Help tab's topics: short answers to what members ask about most.
//! Articles are data one template renders, their lines written in
//! [`crate::markup`] and their numbers taken from what enforces them.

use std::iter;

use crate::{app::App, invite_rules};

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

/// What articles say that depends on the network's settings.
pub struct Facts {
    /// How long the server keeps chat history, in words.
    pub history_kept: String,
    /// Where members open the web client.
    pub webchat: String,
}

impl Facts {
    pub fn new(app: &App) -> Self {
        Self {
            history_kept: invite_rules::in_words(app.history_expiry),
            webchat: app.webchat.redirect_uri(),
        }
    }
}

impl Article {
    pub fn slug(&self) -> &'static str {
        self.topic.href.rsplit('/').next().unwrap_or_default()
    }
}

pub fn articles(facts: &Facts) -> Vec<Article> {
    vec![several_devices(facts)]
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
        related: &[OWN_APP],
    }
}
