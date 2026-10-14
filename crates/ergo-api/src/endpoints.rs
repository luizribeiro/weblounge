use serde::{Deserialize, Serialize};

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::decode;

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
}
