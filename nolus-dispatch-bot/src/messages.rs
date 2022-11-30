use std::any::Any;

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QueryMsg {
    DispatchToAlarms {},
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecuteMsg {
    DispatchAlarms { max_amount: u32 },
}

pub trait Response
where
    Self: for<'de> Deserialize<'de> + Any,
{
    fn remaining_for_dispatch(&self) -> Option<u32>;
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TimeAlarmsResponse {
    NextAlarm {
        /// Timestamp in nanoseconds since the start of the Unix epoch
        unix_time: u64,
    },
    RemainingForDispatch {
        /// `min(remaining_alarms, u32::MAX) as u32`
        remaining_alarms: u32,
    },
}

impl Response for TimeAlarmsResponse {
    fn remaining_for_dispatch(&self) -> Option<u32> {
        if let &Self::RemainingForDispatch { remaining_alarms } = self {
            Some(remaining_alarms)
        } else {
            None
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OracleResponse {
    NoAlarms {},
    RemainingForDispatch {
        /// `min(remaining_alarms, u32::MAX) as u32`
        remaining_alarms: u32,
    },
}

impl Response for OracleResponse {
    fn remaining_for_dispatch(&self) -> Option<u32> {
        if let &Self::RemainingForDispatch { remaining_alarms } = self {
            Some(remaining_alarms)
        } else {
            None
        }
    }
}
