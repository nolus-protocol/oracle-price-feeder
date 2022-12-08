use std::any::Any;

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QueryMsg {
    Status {},
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecuteMsg {
    DispatchAlarms { max_count: u32 },
}

pub trait QueryResponse
where
    Self: DeserializeOwned + Any,
{
    fn remaining_for_dispatch(&self) -> bool;
}

pub trait ExecuteResponse
where
    Self: DeserializeOwned + Any,
{
    fn dispatched_alarms(&self) -> u32;
}

#[derive(Debug, Copy, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct Timestamp(u64);

impl Timestamp {
    pub fn as_nanos(&self) -> u64 {
        self.0
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TimeAlarmsResponse {
    NextAlarm {
        // TODO make sure this is up to date before merging;
        //  additional discussions needed to sync with contracts' side of things
        /// Timestamp in nanoseconds since the start of the Unix epoch
        timestamp: Timestamp,
    },
    RemainingForDispatch {
        /// `min(remaining_alarms, u32::MAX) as u32`
        remaining_alarms: u32,
    },
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct OracleStatusResponse {
    pub remaining_alarms: bool,
}

impl QueryResponse for OracleStatusResponse {
    fn remaining_for_dispatch(&self) -> bool {
        self.remaining_alarms
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct OracleDispatchResponse(pub u32);

impl ExecuteResponse for OracleDispatchResponse {
    fn dispatched_alarms(&self) -> u32 {
        self.0
    }
}
