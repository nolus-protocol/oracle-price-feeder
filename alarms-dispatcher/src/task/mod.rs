use std::{borrow::Cow, sync::Arc};

use anyhow::Result;

use chain_ops::{
    contract::admin::{BaseProtocol, ProtocolContracts},
    supervisor::TaskCreationContext,
    task::{application_defined, NoExpiration, Runnable},
};

use crate::ApplicationDefinedContext;

use self::alarms_generator::{AlarmsGenerator, PriceAlarms, TimeAlarms};

pub mod alarms_generator;

macro_rules! log {
    ($macro:ident!($($body:tt)+)) => {
        ::tracing::$macro!(
            target: "task",
            $($body)+
        );
    };
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum Id {
    TimeAlarmsGenerator,
    PriceAlarmsGenerator { protocol: Arc<str> },
}

impl Id {
    async fn create_time_alarms_task(
        task_creation_context: &mut TaskCreationContext<'_, Task>,
    ) -> Result<Task> {
        task_creation_context
            .admin_contract()
            .platform()
            .await
            .and_then(|platform| {
                alarms_generator::AlarmsGenerator::new_time_alarms(
                    alarms_generator::Configuration {
                        node_client: task_creation_context
                            .node_client()
                            .clone(),
                        transaction_tx: task_creation_context
                            .transaction_tx()
                            .clone(),
                        sender: task_creation_context.signer_address().into(),
                        address: platform.time_alarms.into(),
                        alarms_per_message: task_creation_context
                            .application_defined()
                            .time_alarms_per_message,
                        gas_per_alarm: task_creation_context
                            .application_defined()
                            .gas_per_time_alarm,
                        idle_duration: task_creation_context.idle_duration(),
                        timeout_duration: task_creation_context
                            .timeout_duration(),
                    },
                    TimeAlarms {},
                )
            })
            .map(Task::TimeAlarms)
    }

    async fn create_price_alarms_task(
        mut task_creation_context: TaskCreationContext<'_, Task>,
        protocol_name: Arc<str>,
    ) -> Result<Task> {
        task_creation_context
            .admin_contract()
            .base_protocol(&protocol_name)
            .await
            .and_then(
                |BaseProtocol {
                     contracts: ProtocolContracts { oracle, .. },
                 }| {
                    alarms_generator::AlarmsGenerator::new_price_alarms(
                        alarms_generator::Configuration {
                            node_client: task_creation_context
                                .node_client()
                                .clone(),
                            transaction_tx: task_creation_context
                                .transaction_tx()
                                .clone(),
                            sender: task_creation_context
                                .signer_address()
                                .into(),
                            address: oracle.into(),
                            alarms_per_message: task_creation_context
                                .application_defined()
                                .price_alarms_per_message,
                            gas_per_alarm: task_creation_context
                                .application_defined()
                                .gas_per_price_alarm,
                            idle_duration: task_creation_context
                                .idle_duration(),
                            timeout_duration: task_creation_context
                                .timeout_duration(),
                        },
                        PriceAlarms::new(protocol_name),
                    )
                },
            )
            .map(Task::PriceAlarms)
    }
}

impl application_defined::Id for Id {
    type TaskCreationContext = ApplicationDefinedContext;

    type Task = Task;

    fn protocol(&self) -> Option<&Arc<str>> {
        match self {
            Id::TimeAlarmsGenerator => None,
            Id::PriceAlarmsGenerator { protocol } => Some(protocol),
        }
    }

    fn name(&self) -> Cow<'static, str> {
        match self {
            Id::TimeAlarmsGenerator => Cow::Borrowed("Time Alarms"),
            Id::PriceAlarmsGenerator { protocol } => {
                Cow::Owned(format!("Price Alarms; Protocol={protocol}"))
            },
        }
    }

    async fn into_task(
        self,
        mut task_creation_context: TaskCreationContext<'_, Task>,
    ) -> Result<Task> {
        match self {
            Id::TimeAlarmsGenerator => {
                log!(info!("Creating time alarms generator."));

                Self::create_time_alarms_task(&mut task_creation_context).await
            },
            Id::PriceAlarmsGenerator {
                protocol: protocol_name,
            } => {
                log!(info!(
                    protocol = %protocol_name,
                    "Creating price alarms generator for protocol.",
                ));

                Self::create_price_alarms_task(
                    task_creation_context,
                    protocol_name,
                )
                .await
            },
        }
    }
}

pub enum Task {
    TimeAlarms(AlarmsGenerator<TimeAlarms>),
    PriceAlarms(AlarmsGenerator<PriceAlarms>),
}

impl Runnable for Task {
    async fn run(self) -> Result<()> {
        match self {
            Task::TimeAlarms(alarms_generator) => alarms_generator.run().await,
            Task::PriceAlarms(alarms_generator) => alarms_generator.run().await,
        }
    }
}

impl application_defined::Task for Task {
    type TxExpiration = NoExpiration;

    type Id = Id;

    fn id(&self) -> Id {
        match self {
            Task::TimeAlarms(_) => Id::TimeAlarmsGenerator,
            Task::PriceAlarms(alarms) => Id::PriceAlarmsGenerator {
                protocol: alarms.alarms().protocol().clone(),
            },
        }
    }

    fn protocol_task_set_ids(
        protocol: Arc<str>,
    ) -> impl Iterator<Item = Id> + Send + 'static {
        [Id::PriceAlarmsGenerator { protocol }].into_iter()
    }
}
