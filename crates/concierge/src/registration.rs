//! Telling whether Ergo has an account a registration asked for, when
//! Ergo didn't say it made it: the answer was lost, or the name was
//! taken by an earlier registration whose answer was.

use std::time::Duration;

use ergo_api::{Client, Passphrase};

use crate::name::Name;

/// How many times, [`RECHECK_INTERVAL`] apart, to ask Ergo whether a
/// registration it got no answer to created the account. Each ask can
/// take Ergo's 10s, as checking a password is as slow as setting one, so
/// with the registration's own 10s these answer within 44s, inside the
/// 60s nginx waits for a page.
pub const LOST_REPLY_CHECKS: usize = 3;
const RECHECK_INTERVAL: Duration = Duration::from_secs(2);

/// The Ergo account `name` as Ergo spells it, if `password` opens it,
/// itself rather than an account `name` is grouped to. Asks Ergo up to
/// `checks` times, [`RECHECK_INTERVAL`] apart, while it doesn't answer or
/// has no such account, as a registration may still be finishing.
pub async fn opened<K>(
    ergo: &Client,
    name: &Name<K>,
    password: &Passphrase,
    checks: usize,
) -> ergo_api::Result<Option<Name<K>>> {
    let mut opened = Ok(None);
    for check in 0..checks {
        if check > 0 {
            tokio::time::sleep(RECHECK_INTERVAL).await;
        }
        opened = ergo.check_auth(name, password.expose()).await;
        if matches!(opened, Ok(Some(_))) {
            break;
        }
    }
    Ok(opened?
        .map(|opened| Name::new(opened.as_str()))
        .filter(|opened| opened == name))
}

#[cfg(test)]
mod tests {
    use ergo_api::{
        Registration,
        fake::{Failure, FakeErgo},
    };

    use super::*;
    use crate::name::BotName;

    fn password() -> Passphrase {
        "correct-horse-battery".parse().unwrap()
    }

    async fn with_bender() -> FakeErgo {
        let ergo = FakeErgo::start().await;
        let registered = ergo.client().register("Bender", &password()).await;
        assert_eq!(registered.unwrap(), Registration::Created);
        ergo
    }

    #[tokio::test]
    async fn answers_the_account_as_ergo_spells_it_when_the_password_opens_it() {
        let ergo = with_bender().await;

        let opened = opened(&ergo.client(), &BotName::new("bender"), &password(), 1).await;

        assert_eq!(opened.unwrap().unwrap().as_str(), "Bender");
    }

    #[tokio::test]
    async fn answers_none_for_a_wrong_password_or_a_missing_account() {
        let ergo = with_bender().await;
        let wrong = "wrong-horse-battery".parse().unwrap();
        let client = ergo.client();

        assert_eq!(
            opened(&client, &BotName::new("bender"), &wrong, 1)
                .await
                .unwrap(),
            None
        );
        assert_eq!(
            opened(&client, &BotName::new("flexo"), &password(), 1)
                .await
                .unwrap(),
            None
        );
    }

    #[tokio::test]
    async fn asks_again_while_ergo_does_not_answer() {
        let ergo = with_bender().await;
        ergo.fail_next(Failure::Dropped);

        let opened = opened(&ergo.client(), &BotName::new("bender"), &password(), 2).await;

        assert!(opened.unwrap().is_some());
    }

    #[tokio::test]
    async fn answers_the_last_failure_once_out_of_checks() {
        let ergo = with_bender().await;
        ergo.fail_next(Failure::Dropped);

        let error = opened(&ergo.client(), &BotName::new("bender"), &password(), 1).await;

        assert!(error.unwrap_err().unanswered());
    }
}
