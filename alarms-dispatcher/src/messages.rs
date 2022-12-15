use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QueryMsg {
    AlarmsStatus {},
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
    pub fn remaining_for_dispatch(&self) -> bool {
        self.remaining_alarms
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct DispatchResponse(pub u32);

impl DispatchResponse {
    pub fn dispatched_alarms(&self) -> u32 {
        self.0
    }
}
