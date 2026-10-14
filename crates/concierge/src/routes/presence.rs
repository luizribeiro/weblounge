//! Ergo's always-on and auto-away, as two switches on the account home.
//!
//! Ergo keeps each as default, on or off, and our network's default for
//! both is on (opt-out in services/ergo.nix). So default reads as on, and
//! switching one on goes back to the default rather than pinning it.

use axum::{
    Form,
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Redirect, Response},
};
use ergo_api::{Preference, Settings, SettingsChanges};
use serde::Deserialize;

use super::{
    ERGO_UNAVAILABLE,
    account::{self, Forms},
};
use crate::{app::AppState, auth::CurrentMember};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Presence {
    pub always_on: bool,
    pub auto_away: bool,
}

impl From<&Settings> for Presence {
    fn from(settings: &Settings) -> Self {
        Self {
            always_on: is_on(settings.always_on),
            auto_away: is_on(settings.auto_away),
        }
    }
}

impl Presence {
    fn changes(self) -> SettingsChanges {
        SettingsChanges {
            always_on: Some(preference(self.always_on)),
            auto_away: Some(preference(self.auto_away)),
            ..SettingsChanges::default()
        }
    }
}

fn is_on(preference: Preference) -> bool {
    preference != Preference::Off
}

fn preference(on: bool) -> Preference {
    if on {
        Preference::Default
    } else {
        Preference::Off
    }
}

/// Unchecked boxes send nothing.
#[derive(Deserialize, Default)]
#[serde(default)]
pub struct Switches {
    always_on: Option<String>,
    auto_away: Option<String>,
}

pub async fn update(
    State(app): State<AppState>,
    CurrentMember(me): CurrentMember,
    Form(switches): Form<Switches>,
) -> Response {
    let presence = Presence {
        always_on: switches.always_on.is_some(),
        auto_away: switches.auto_away.is_some(),
    };
    let account = me.member.account.as_str();
    match app.ergo.update_settings(account, presence.changes()).await {
        Ok(_) => Redirect::to("/?irc#irc-settings").into_response(),
        Err(error) => {
            eprintln!("Changing IRC settings failed: {error}");
            let forms = Forms::presence_error(ERGO_UNAVAILABLE);
            account::home(&app, &me.member, StatusCode::SERVICE_UNAVAILABLE, forms).await
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings(always_on: Preference, auto_away: Preference) -> Settings {
        Settings {
            always_on,
            auto_away,
            email: None,
            replay_joins: ergo_api::ReplayJoins::CommandsOnly,
        }
    }

    #[test]
    fn only_off_reads_as_off() {
        for (preference, on) in [
            (Preference::Default, true),
            (Preference::On, true),
            (Preference::Off, false),
        ] {
            let presence = Presence::from(&settings(preference, Preference::Off));
            assert_eq!(presence.always_on, on, "{preference:?}");
            assert!(!presence.auto_away);
        }
    }

    #[test]
    fn on_goes_back_to_the_default_and_off_is_off() {
        let changes = Presence {
            always_on: true,
            auto_away: false,
        }
        .changes();
        assert_eq!(changes.always_on, Some(Preference::Default));
        assert_eq!(changes.auto_away, Some(Preference::Off));
        assert_eq!((changes.email, changes.replay_joins), (None, None));
    }
}
