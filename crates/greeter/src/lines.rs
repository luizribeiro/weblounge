//! What the greeter says. Plain lines, naming people by their nicks and
//! never guessing at pronouns.

use concierge::name::MemberName;

/// The welcome in #lounge, naming the inviter unless a root invite, or a
/// member removed since, brought the newcomer.
pub fn welcome(newcomer: &MemberName, inviter: Option<&MemberName>) -> String {
    match inviter {
        Some(inviter) => format!("Welcome {newcomer}, invited by {inviter}!"),
        None => format!("Welcome {newcomer}!"),
    }
}

/// The direct message to the inviter, once their invitee was welcomed.
pub fn inviter_told(newcomer: &MemberName) -> String {
    format!(
        "{newcomer} accepted your invite and was welcomed in #lounge. \
         Say hi when you get a chance."
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_welcome_names_the_inviter_when_there_is_one() {
        let erin = MemberName::new("erin");
        let luiz = MemberName::new("luiz");
        assert_eq!(
            welcome(&erin, Some(&luiz)),
            "Welcome erin, invited by luiz!"
        );
        assert_eq!(welcome(&erin, None), "Welcome erin!");
    }
}
