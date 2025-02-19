use anyhow::{Context as _, Result};
use cosmrs::Gas;

use environment::ReadFromVar as _;
use service::run_app;

mod task;

run_app!(
    task_creation_context: {
        Ok(ApplicationDefinedContext {
            gas_per_time_alarm: read_gas_per_time_alarm()?,
            time_alarms_per_message: read_time_alarms_per_message()?,
            gas_per_price_alarm: read_gas_per_price_alarm()?,
            price_alarms_per_message: read_price_alarms_per_message()?,
        })
    },
    startup_tasks: [task::Id::TimeAlarmsGenerator].into_iter(),
);

pub struct ApplicationDefinedContext {
    pub gas_per_time_alarm: Gas,
    pub time_alarms_per_message: u32,
    pub gas_per_price_alarm: Gas,
    pub price_alarms_per_message: u32,
}

fn read_gas_per_time_alarm() -> Result<Gas> {
    Gas::read_from_var("TIME_ALARMS_GAS_LIMIT_PER_ALARM")
        .context("Failed to read gas limit per time alarm!")
}

fn read_time_alarms_per_message() -> Result<u32> {
    u32::read_from_var("TIME_ALARMS_MAX_ALARMS_GROUP")
        .context("Failed to read maximum count of time alarms per message!")
}

fn read_gas_per_price_alarm() -> Result<Gas> {
    Gas::read_from_var("PRICE_ALARMS_GAS_LIMIT_PER_ALARM")
        .context("Failed to read gas limit per price alarm!")
}

fn read_price_alarms_per_message() -> Result<u32> {
    u32::read_from_var("PRICE_ALARMS_MAX_ALARMS_GROUP")
        .context("Failed to read maximum count of price alarms per message!")
}
