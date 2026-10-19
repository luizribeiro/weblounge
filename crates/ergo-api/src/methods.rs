use crate::{
    Account, AccountName, AccountSummary, CheckAuth, Client, Erase, Error, ErrorCode, HsSet, Info,
    List, Passphrase, Passwd, Result, SaGet, SaRegister, SaSet, ServerStatus, Settings,
    SettingsChanges, Status,
};

/// What came of [`Client::register`].
#[must_use]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Registration {
    Created,
    /// An account already holds the name, in any case.
    NameTaken,
}

/// What came of [`Client::erase`].
#[must_use]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Erasure {
    Erased,
    /// Ergo has no record of the name, maybe because it was erased already.
    NoSuchAccount,
}

impl Client {
    /// The server's version and population.
    pub async fn status(&self) -> Result<ServerStatus> {
        self.call(&Status).await
    }

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

    /// Replaces an account's passphrase.
    pub async fn set_password(
        &self,
        account_name: impl Into<AccountName>,
        passphrase: &Passphrase,
    ) -> Result<()> {
        let request = Passwd {
            account_name: account_name.into(),
            passphrase: passphrase.clone(),
        };
        self.call(&request).await.map(drop)
    }

    /// Deletes an account and disconnects its clients, unless Ergo has no
    /// record of it. Its name is free again as far as Ergo knows.
    pub async fn erase(&self, account_name: impl Into<AccountName>) -> Result<Erasure> {
        let request = Erase {
            account_name: account_name.into(),
        };
        match self.call(&request).await {
            Ok(_) => Ok(Erasure::Erased),
            Err(error) if error.code() == Some(&ErrorCode::AccountDoesNotExist) => {
                Ok(Erasure::NoSuchAccount)
            }
            Err(error) => Err(error),
        }
    }

    /// An account's details, or `None` if it doesn't exist, isn't verified
    /// or is suspended: Ergo answers all three alike.
    pub async fn info(&self, account_name: impl Into<AccountName>) -> Result<Option<Account>> {
        let request = Info {
            account_name: account_name.into(),
        };
        unless_bare_refusal(self.call(&request).await)
    }

    /// Every account.
    pub async fn accounts(&self) -> Result<Vec<AccountSummary>> {
        Ok(self.call(&List).await?.accounts)
    }

    /// An account's settings.
    pub async fn settings(&self, account_name: impl Into<AccountName>) -> Result<Settings> {
        let request = SaGet {
            account_name: account_name.into(),
        };
        self.call(&request).await
    }

    /// Applies `changes` to an account's settings and returns all of them.
    pub async fn update_settings(
        &self,
        account_name: impl Into<AccountName>,
        changes: SettingsChanges,
    ) -> Result<Settings> {
        let request = SaSet {
            account_name: account_name.into(),
            changes,
        };
        self.call(&request).await
    }

    /// Sets an account's vhost, the host everyone sees it connect from.
    pub async fn set_vhost(
        &self,
        account_name: impl Into<AccountName>,
        vhost: impl Into<String>,
    ) -> Result<()> {
        let request = HsSet {
            account_name: account_name.into(),
            vhost: vhost.into(),
        };
        self.call(&request).await.map(drop)
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
    use crate::{Preference, ReplayJoins, UserCounts, stub::Stub};

    fn passphrase() -> Passphrase {
        "q7Hx-2mZp_9vLcT4wRfB".parse().unwrap()
    }

    fn assert_sent(stub: &Stub, path: &str, body: Value) {
        let [request] = stub.requests().try_into().unwrap();
        assert_eq!((request.path.as_str(), request.body), (path, body));
    }

    #[tokio::test]
    async fn status_reports_the_servers_version_and_population() {
        let stub = Stub::answering(
            200,
            r#"{"success":true,"version":"2.19.1","go_version":"go1.26.8","start_time":"2026-10-14T03:23:56.300Z","users":{"total":3,"invisible":3,"operators":0,"unknown":0,"max":5},"channels":1,"servers":1}"#,
        )
        .await;
        let status = stub.client().status().await.unwrap();
        assert_eq!(status.users, UserCounts { total: 3, max: 5 });
        assert_sent(&stub, "/v1/status", Value::Null);
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
    async fn set_password_replaces_the_passphrase() {
        let stub = Stub::answering(200, r#"{"success":true}"#).await;
        stub.client()
            .set_password("alice", &passphrase())
            .await
            .unwrap();
        assert_sent(
            &stub,
            "/v1/ns/passwd",
            json!({"accountName": "alice", "passphrase": "q7Hx-2mZp_9vLcT4wRfB"}),
        );
    }

    #[tokio::test]
    async fn set_password_fails_for_an_unknown_account() {
        let stub = Stub::answering(
            200,
            r#"{"success":false,"errorCode":"ACCOUNT_DOES_NOT_EXIST"}"#,
        )
        .await;
        let error = stub
            .client()
            .set_password("nobody", &passphrase())
            .await
            .unwrap_err();
        assert_eq!(error.code(), Some(&ErrorCode::AccountDoesNotExist));
    }

    #[tokio::test]
    async fn set_vhost_sends_the_account_and_its_vhost() {
        let stub = Stub::answering(200, r#"{"success":true}"#).await;
        stub.client()
            .set_vhost("louro", "weblounge/bot/luiz")
            .await
            .unwrap();
        assert_sent(
            &stub,
            "/v1/hs/set",
            json!({"accountName": "louro", "vhost": "weblounge/bot/luiz"}),
        );
    }

    #[tokio::test]
    async fn set_vhost_fails_for_a_vhost_ergo_refuses() {
        let stub = Stub::answering(200, r#"{"success":false,"errorCode":"INVALID_VHOST"}"#).await;
        let error = stub
            .client()
            .set_vhost("louro", "bot host")
            .await
            .unwrap_err();
        assert_eq!(error.code(), Some(&ErrorCode::InvalidVhost));
    }

    #[tokio::test]
    async fn erase_sends_the_account() {
        let stub = Stub::answering(200, r#"{"success":true}"#).await;
        let erasure = stub.client().erase("bender").await.unwrap();
        assert_eq!(erasure, Erasure::Erased);
        assert_sent(&stub, "/v1/ns/erase", json!({"accountName": "bender"}));
    }

    #[tokio::test]
    async fn erase_reports_an_account_ergo_has_no_record_of() {
        let stub = Stub::answering(
            200,
            r#"{"success":false,"errorCode":"ACCOUNT_DOES_NOT_EXIST"}"#,
        )
        .await;
        let erasure = stub.client().erase("nobody").await.unwrap();
        assert_eq!(erasure, Erasure::NoSuchAccount);
    }

    #[tokio::test]
    async fn erase_passes_other_refusals_on() {
        let stub = Stub::answering(200, r#"{"success":false,"errorCode":"INVALID_REQUEST"}"#).await;
        let error = stub.client().erase("").await.unwrap_err();
        assert_eq!(error.code(), Some(&ErrorCode::InvalidRequest));
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

    #[tokio::test]
    async fn accounts_lists_every_account() {
        let stub = Stub::answering(
            200,
            r#"{"success":true,"accounts":[{"success":true,"accountName":"Alice"}],"totalCount":1}"#,
        )
        .await;
        let accounts = stub.client().accounts().await.unwrap();
        assert_eq!(
            accounts,
            [AccountSummary {
                account_name: "Alice".into(),
                email: None
            }]
        );
        assert_sent(&stub, "/v1/ns/list", Value::Null);
    }

    #[tokio::test]
    async fn settings_answers_with_the_accounts_settings() {
        let stub = Stub::answering(
            200,
            r#"{"success":true,"alwaysOn":"on","autoAway":"default","replayJoins":"on"}"#,
        )
        .await;
        let settings = stub.client().settings("alice").await.unwrap();
        assert_eq!(settings.replay_joins, ReplayJoins::On);
        assert_sent(&stub, "/v1/ns/saget", json!({"accountName": "alice"}));
    }

    #[tokio::test]
    async fn update_settings_sends_the_changes_and_answers_with_every_setting() {
        let stub = Stub::answering(
            200,
            r#"{"success":true,"alwaysOn":"off","autoAway":"default","replayJoins":"commands-only"}"#,
        )
        .await;
        let changes = SettingsChanges {
            always_on: Some(Preference::Off),
            ..Default::default()
        };

        let settings = stub
            .client()
            .update_settings("alice", changes)
            .await
            .unwrap();

        assert_eq!(settings.always_on, Preference::Off);
        assert_sent(
            &stub,
            "/v1/ns/saset",
            json!({"accountName": "alice", "alwaysOn": "off"}),
        );
    }
}
