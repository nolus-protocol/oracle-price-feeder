use serde::{Deserialize, Serialize};

pub(crate) enum QueryMsg {}

impl QueryMsg {
    pub const CONTRACT_VERSION: &'static [u8] = br#"{"contract_version":{}}"#;

    pub const ALARMS_STATUS: &'static [u8] = br#"{"alarms_status":{}}"#;
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecuteMsg {
    DispatchAlarms { max_count: u32 },
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct StatusResponse {
    pub remaining_alarms: bool,
}

impl StatusResponse {
    #[must_use]
    pub const fn remaining_for_dispatch(&self) -> bool {
        self.remaining_alarms
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct DispatchResponse(u32);

impl DispatchResponse {
    #[must_use]
    pub const fn dispatched_alarms(&self) -> u32 {
        self.0
    }
}
