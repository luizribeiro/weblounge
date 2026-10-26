//! What the greeter says. Plain lines, naming people by their nicks and
//! never guessing at pronouns.

use concierge::{clients::GUIDES, name::MemberName, origin::SiteOrigin};

/// The welcome in #lounge.
pub fn welcome(newcomer: &MemberName, inviter: &MemberName) -> String {
    format!("Welcome {newcomer}, invited by {inviter}!")
}

/// The direct message to the inviter, once their invitee was welcomed.
pub fn inviter_told(newcomer: &MemberName) -> String {
    format!(
        "{newcomer} accepted your invite and was welcomed in #lounge. \
         Say hi when you get a chance."
    )
}

/// The one direct message to the inviter of a newcomer who hasn't
/// connected two days in.
pub fn nudge(newcomer: &MemberName) -> String {
    format!(
        "{newcomer} hasn't connected since accepting your invite. \
         A nudge from you might help."
    )
}

/// The newcomer's one direct message, the first time they're seen
/// connected, each line sent on its own.
pub fn orientation(newcomer: &MemberName, public_url: &SiteOrigin) -> [String; 2] {
    [
        format!("Hi {newcomer}, I'm the concierge, weblounge's host."),
        format!("Get pinged when someone replies: {public_url}{GUIDES}"),
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
}
