use crate::{
    Account, AccountName, CheckAuth, Client, Error, ErrorCode, Info, Passphrase, Result, SaRegister,
};

/// What came of [`Client::register`].
#[must_use]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Registration {
    Created,
    /// An account already holds the name, in any case.
    NameTaken,
}

impl Client {
    /// Creates a verified account, unless the name is taken.
    pub async fn register(
        &self,
        account_name: impl Into<AccountName>,
        passphrase: &Passphrase,
    ) -> Result<Registration> {
        let request = SaRegister {
            account_name: account_name.into(),
            passphrase: passphrase.clone(),
        };
        match self.call(&request).await {
            Ok(_) => Ok(Registration::Created),
            Err(error) if error.code() == Some(&ErrorCode::AccountExists) => {
                Ok(Registration::NameTaken)
            }
            Err(error) => Err(error),
        }
    }

    /// The canonical name of the account `passphrase` opens, or `None` when
    /// the account doesn't exist or the passphrase is wrong.
    pub async fn check_auth(
        &self,
        account_name: impl Into<AccountName>,
        passphrase: &str,
    ) -> Result<Option<AccountName>> {
        let request = CheckAuth {
            account_name: account_name.into(),
            passphrase: passphrase.to_owned(),
        };
        let authenticated = unless_bare_refusal(self.call(&request).await)?;
        Ok(authenticated.map(|authenticated| authenticated.account_name))
    }

    /// An account's details, or `None` if it doesn't exist, isn't verified
    /// or is suspended: Ergo answers all three alike.
    pub async fn info(&self, account_name: impl Into<AccountName>) -> Result<Option<Account>> {
        let request = Info {
            account_name: account_name.into(),
        };
        unless_bare_refusal(self.call(&request).await)
    }
}

/// Ergo answers a bare `{"success":false}` when there's simply nothing to
/// return, keeping codes and messages for real failures.
fn unless_bare_refusal<T>(result: Result<T>) -> Result<Option<T>> {
    match result {
        Ok(value) => Ok(Some(value)),
        Err(Error::Refused {
            code: None,
            message: None,
        }) => Ok(None),
        Err(error) => Err(error),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::*;
    use crate::stub::Stub;

    fn passphrase() -> Passphrase {
        "q7Hx-2mZp_9vLcT4wRfB".parse().unwrap()
    }

    fn assert_sent(stub: &Stub, path: &str, body: Value) {
        let [request] = stub.requests().try_into().unwrap();
        assert_eq!((request.path.as_str(), request.body), (path, body));
    }

    #[tokio::test]
    async fn register_creates_the_account() {
        let stub = Stub::answering(200, r#"{"success":true}"#).await;
        let registration = stub.client().register("alice", &passphrase()).await;
        assert_eq!(registration.unwrap(), Registration::Created);
        assert_sent(
            &stub,
            "/v1/ns/saregister",
            json!({"accountName": "alice", "passphrase": "q7Hx-2mZp_9vLcT4wRfB"}),
        );
    }

    #[tokio::test]
    async fn register_reports_a_taken_name() {
        let stub = Stub::answering(
            200,
            r#"{"success":false,"error":"Name reserved due to a prior registration","errorCode":"ACCOUNT_EXISTS"}"#,
        )
        .await;
        let registration = stub.client().register("Alice", &passphrase()).await;
        assert_eq!(registration.unwrap(), Registration::NameTaken);
    }

    #[tokio::test]
    async fn register_passes_other_refusals_on() {
        let stub = Stub::answering(
            200,
            r#"{"success":false,"error":"Account could not be created","errorCode":"UNKNOWN_ERROR"}"#,
        )
        .await;
        let error = stub
            .client()
            .register("a b", &passphrase())
            .await
            .unwrap_err();
        assert_eq!(error.code(), Some(&ErrorCode::UnknownError));
    }

    #[tokio::test]
    async fn check_auth_answers_with_the_canonical_name() {
        let stub = Stub::answering(200, r#"{"success":true,"accountName":"Alice"}"#).await;
        let account = stub.client().check_auth("ALICE", "hunter2").await.unwrap();
        assert_eq!(account, Some("Alice".into()));
        assert_sent(
            &stub,
            "/v1/check_auth",
            json!({"accountName": "ALICE", "passphrase": "hunter2"}),
        );
    }

    #[tokio::test]
    async fn check_auth_answers_none_for_wrong_credentials() {
        let stub = Stub::answering(200, r#"{"success":false}"#).await;
        let account = stub.client().check_auth("alice", "wrong").await.unwrap();
        assert_eq!(account, None);
    }

    #[tokio::test]
    async fn check_auth_fails_when_ergo_reports_an_error() {
        let stub = Stub::answering(200, r#"{"success":false,"error":"database is closed"}"#).await;
        let error = stub.client().check_auth("alice", "hunter2").await;
        assert!(matches!(error, Err(Error::Refused { .. })), "{error:?}");
    }

    #[tokio::test]
    async fn info_answers_with_the_accounts_details() {
        let stub = Stub::answering(200, r#"{"success":true,"accountName":"Alice"}"#).await;
        let account = stub.client().info("alice").await.unwrap().unwrap();
        assert_eq!(account.account_name, "Alice".into());
        assert_sent(&stub, "/v1/ns/info", json!({"accountName": "alice"}));
    }

    #[tokio::test]
    async fn info_answers_none_for_an_unknown_account() {
        let stub = Stub::answering(200, r#"{"success":false}"#).await;
        assert_eq!(stub.client().info("nobody").await.unwrap(), None);
    }
}
