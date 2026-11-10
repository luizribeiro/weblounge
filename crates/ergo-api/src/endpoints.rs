use secret::SecretString;
use serde::{Deserialize, Serialize};

use crate::{AccountName, Passphrase};

/// `/v1/status`: the server's version and population.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Status;
endpoint!(Status, "/v1/status" => ServerStatus);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServerStatus {
    pub version: String,
    /// When the server started, in ISO 8601.
    pub start_time: String,
    pub users: UserCounts,
    pub channels: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UserCounts {
    /// Users connected now.
    pub total: u64,
    /// The most users connected at once since the server started.
    pub max: u64,
}

/// The empty answer of endpoints that only report success.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Ack {}

/// `/v1/ns/saregister`: creates a verified account.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaRegister {
    pub account_name: AccountName,
    pub passphrase: Passphrase,
}
endpoint!(SaRegister, "/v1/ns/saregister" => Ack);

/// `/v1/check_auth`: checks an account's passphrase. The passphrase is
/// whatever someone typed, so it isn't a [`Passphrase`], only a secret.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CheckAuth {
    pub account_name: AccountName,
    #[serde(serialize_with = "secret::serialize_exposed")]
    pub passphrase: SecretString,
}
endpoint!(CheckAuth, "/v1/check_auth" => Authenticated);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Authenticated {
    /// The account's canonical spelling.
    pub account_name: AccountName,
}

/// `/v1/ns/passwd`: replaces an account's passphrase.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Passwd {
    pub account_name: AccountName,
    pub passphrase: Passphrase,
}
endpoint!(Passwd, "/v1/ns/passwd" => Ack);

/// `/v1/ns/erase`: deletes every record of an account and disconnects
/// the clients logged in to it, as an oper's `NS ERASE` does. Ours,
/// patched into Ergo.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Erase {
    pub account_name: AccountName,
}
endpoint!(Erase, "/v1/ns/erase" => Ack);

/// `/v1/ns/info`: one account's details.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Info {
    pub account_name: AccountName,
}
endpoint!(Info, "/v1/ns/info" => Account);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Account {
    /// The account's canonical spelling.
    pub account_name: AccountName,
    /// When the account was registered, in ISO 8601.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub registered_at: Option<String>,
    /// The channels the account is in.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub channels: Vec<String>,
}

/// `/v1/ns/list`: every account.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct List;
endpoint!(List, "/v1/ns/list" => AccountList);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccountList {
    pub accounts: Vec<AccountSummary>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountSummary {
    /// The account's canonical spelling.
    pub account_name: AccountName,
}

/// `/v1/hs/set`: sets an account's vhost, or clears it when empty. Ours,
/// patched into Ergo.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HsSet {
    pub account_name: AccountName,
    pub vhost: String,
}
endpoint!(HsSet, "/v1/hs/set" => Ack);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::decode;
    use serde_json::json;

    #[test]
    fn decodes_ergos_status() {
        let status: ServerStatus = decode(
            r#"{"success":true,"version":"2.19.1","go_version":"go1.26.8","start_time":"2026-10-14T03:23:56.300Z","users":{"total":3,"invisible":3,"operators":0,"unknown":0,"max":5},"channels":1,"servers":1}"#,
        )
        .unwrap();
        assert_eq!(
            status,
            ServerStatus {
                version: "2.19.1".into(),
                start_time: "2026-10-14T03:23:56.300Z".into(),
                users: UserCounts { total: 3, max: 5 },
                channels: 1,
            }
        );
    }

    #[test]
    fn requests_use_ergos_field_names() {
        let request = SaRegister {
            account_name: "alice".into(),
            passphrase: "q7Hx-2mZp_9vLcT4wRfB".parse().unwrap(),
        };
        assert_eq!(
            serde_json::to_value(request).unwrap(),
            json!({"accountName": "alice", "passphrase": "q7Hx-2mZp_9vLcT4wRfB"})
        );
    }

    #[test]
    fn debug_output_hides_a_typed_passphrase() {
        let request = CheckAuth {
            account_name: "alice".into(),
            passphrase: "hunter 2".into(),
        };
        assert_eq!(
            format!("{request:?}"),
            r#"CheckAuth { account_name: AccountName("alice"), passphrase: <redacted> }"#
        );
    }

    #[test]
    fn decodes_ergos_account_info() {
        let account: Account = decode(
            r##"{"success":true,"accountName":"Alice","registeredAt":"2026-10-14T03:23:57.810Z","channels":["#lounge"]}"##,
        )
        .unwrap();
        assert_eq!(
            account,
            Account {
                account_name: "Alice".into(),
                registered_at: Some("2026-10-14T03:23:57.810Z".into()),
                channels: vec!["#lounge".into()],
            }
        );
    }

    #[test]
    fn decodes_an_account_without_optional_details() {
        let account: Account = decode(r#"{"success":true,"accountName":"Alice"}"#).unwrap();
        assert_eq!(
            account,
            Account {
                account_name: "Alice".into(),
                registered_at: None,
                channels: Vec::new(),
            }
        );
    }

    #[test]
    fn decodes_ergos_account_list_ignoring_its_undocumented_emails() {
        let list: AccountList = decode(
            r#"{"success":true,"accounts":[{"success":true,"accountName":"Alice","email":"a@example.org"},{"success":true,"accountName":"bob"}],"totalCount":2}"#,
        )
        .unwrap();
        assert_eq!(
            list.accounts,
            [
                AccountSummary {
                    account_name: "Alice".into(),
                },
                AccountSummary {
                    account_name: "bob".into(),
                },
            ]
        );
    }
}
