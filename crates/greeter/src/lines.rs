//! What the greeter says. Plain lines, naming people by their nicks and
//! never guessing at pronouns.

use concierge::{clients::GUIDES, name::MemberName, origin::SiteOrigin};

/// The welcome in #lounge.
pub fn welcome(newcomer: &MemberName, inviter: &MemberName) -> String {
    format!("Welcome {newcomer}, invited by {inviter}!")
}

/// The direct message to the inviter, once their invitee was welcomed.
pub fn inviter_told(newcomer: &MemberName) -> String {
    format!("{newcomer} just connected for the first time and was welcomed in #lounge. Say hi!")
}

/// The one direct message to the inviter of a newcomer who hasn't
/// connected two days in.
pub fn nudge(newcomer: &MemberName) -> String {
    format!(
        "{newcomer} hasn't connected since accepting your invite. \
         A nudge from you might help."
    )
}

/// The newcomer's hello, the first time they're seen connected, each line
/// a message of its own.
pub fn hello(newcomer: &MemberName, inviter: &MemberName, public_url: &SiteOrigin) -> Vec<String> {
    vec![
        format!("Hi {newcomer}! I'm concierge, weblounge's resident bot. Welcome aboard."),
        "Make sure notifications are on, so you hear when someone mentions you. On the \
         webchat, use the \"Turn on desktop notifications\" link at the top or Settings, if \
         your browser supports it."
            .to_owned(),
        format!(
            "Want weblounge on your phone or desktop? These apps work well here: \
             {public_url}{GUIDES}"
        ),
        format!(
            "I can't chat yet. Questions? Ask {inviter}, who invited you, or anyone in #lounge."
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_welcome_names_the_inviter() {
        let erin = MemberName::new("erin");
        let luiz = MemberName::new("luiz");
        assert_eq!(welcome(&erin, &luiz), "Welcome erin, invited by luiz!");
    }

    #[test]
    fn the_hello_points_at_notifications_the_apps_and_the_inviter() {
        let public_url = SiteOrigin::parse("https://members.weblounge.org").unwrap();
        let erin = MemberName::new("erin");
        assert_eq!(
            hello(&erin, &MemberName::new("luiz"), &public_url),
            [
                "Hi erin! I'm concierge, weblounge's resident bot. Welcome aboard.",
                "Make sure notifications are on, so you hear when someone mentions you. \
                 On the webchat, use the \"Turn on desktop notifications\" link at the top \
                 or Settings, if your browser supports it.",
                "Want weblounge on your phone or desktop? These apps work well here: \
                 https://members.weblounge.org/help/apps",
                "I can't chat yet. Questions? Ask luiz, who invited you, or anyone in #lounge.",
            ]
        );
    }
}
