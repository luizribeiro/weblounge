//! Ergo's always-on and auto-away, as two switches on the Account tab.
//!
//! Ergo keeps each as default, on or off, and default follows the
//! network's multiclient settings: on under opt-out, off under opt-in.
//! Switching one to match the network goes back to default rather than
//! pinning it, so the member keeps following the network.

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

impl Presence {
    /// What a member's `settings` come to on a network with `defaults`.
    pub fn of(settings: &Settings, defaults: Self) -> Self {
        Self {
            always_on: resolve(settings.always_on, defaults.always_on),
            auto_away: resolve(settings.auto_away, defaults.auto_away),
        }
    }

    fn changes(self, defaults: Self) -> SettingsChanges {
        SettingsChanges {
            always_on: Some(preference(self.always_on, defaults.always_on)),
            auto_away: Some(preference(self.auto_away, defaults.auto_away)),
            ..SettingsChanges::default()
        }
    }

    /// The line under the switches, for the network's defaults.
    pub fn hint(self) -> &'static str {
        match (self.always_on, self.auto_away) {
            (true, true) => "Both on by default.",
            (false, false) => "Both off by default.",
            (true, false) => "Always on is on by default, auto-away off.",
            (false, true) => "Always on is off by default, auto-away on.",
        }
    }
}

fn resolve(preference: Preference, default: bool) -> bool {
    match preference {
        Preference::Default => default,
        Preference::On => true,
        Preference::Off => false,
    }
}

fn preference(on: bool, default: bool) -> Preference {
    if on == default {
        Preference::Default
    } else if on {
        Preference::On
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
    let changes = presence.changes(app.presence_defaults);
    match app.ergo.update_settings(account, changes).await {
        Ok(_) => Redirect::to("/account?irc#irc-settings").into_response(),
        Err(error) => {
            eprintln!("Changing IRC settings failed: {error}");
            let status = StatusCode::SERVICE_UNAVAILABLE;
            let forms = Forms::irc_error(ERGO_UNAVAILABLE);
            account::page(&app, &me.member, status, forms).await
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const OPT_OUT: Presence = Presence {
        always_on: true,
        auto_away: true,
    };
    const OPT_IN: Presence = Presence {
        always_on: false,
        auto_away: false,
    };

    fn settings(always_on: Preference, auto_away: Preference) -> Settings {
        Settings {
            always_on,
            auto_away,
            email: None,
            replay_joins: ergo_api::ReplayJoins::CommandsOnly,
        }
    }

    #[test]
    fn default_reads_as_the_networks_and_a_choice_as_itself() {
        for (defaults, default_reads_as) in [(OPT_OUT, true), (OPT_IN, false)] {
            for (preference, on) in [
                (Preference::Default, default_reads_as),
                (Preference::On, true),
                (Preference::Off, false),
            ] {
                let presence = Presence::of(&settings(preference, Preference::Default), defaults);
                assert_eq!(presence.always_on, on, "{preference:?} on {defaults:?}");
                assert_eq!(presence.auto_away, default_reads_as, "{defaults:?}");
            }
        }
    }

    #[test]
    fn mixed_defaults_resolve_each_setting_on_its_own() {
        let defaults = Presence {
            always_on: true,
            auto_away: false,
        };
        let presence = Presence::of(
            &settings(Preference::Default, Preference::Default),
            defaults,
        );
        assert_eq!(presence, defaults);
    }

    #[test]
    fn matching_the_network_goes_back_to_default_and_differing_pins_it() {
        let wanted = Presence {
            always_on: true,
            auto_away: false,
        };
        for (defaults, expected) in [
            (OPT_OUT, (Preference::Default, Preference::Off)),
            (OPT_IN, (Preference::On, Preference::Default)),
        ] {
            let changes = wanted.changes(defaults);
            let written = (changes.always_on.unwrap(), changes.auto_away.unwrap());
            assert_eq!(written, expected, "{defaults:?}");
            assert_eq!((changes.email, changes.replay_joins), (None, None));
        }
    }

    #[test]
    fn the_hint_names_each_default() {
        assert_eq!(OPT_OUT.hint(), "Both on by default.");
        assert_eq!(OPT_IN.hint(), "Both off by default.");
        let mixed = Presence {
            always_on: false,
            auto_away: true,
        };
        assert_eq!(mixed.hint(), "Always on is off by default, auto-away on.");
    }
}
