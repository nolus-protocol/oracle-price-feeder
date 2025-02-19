use std::{borrow::Cow, sync::Arc};

use anyhow::Result;

use contract::{GeneralizedProtocol, GeneralizedProtocolContracts, Platform};
use service::{
    supervisor::configuration,
    task::{
        application_defined, NoExpiration, Runnable, RunnableState, TxPackage,
    },
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
        service_configuration: &configuration::Service,
        task_creation_context: &ApplicationDefinedContext,
        transaction_tx: &channel::unbounded::Sender<
            TxPackage<<Task as application_defined::Task>::TxExpiration>,
        >,
    ) -> Result<Task> {
        let Platform { time_alarms } = service_configuration
            .admin_contract()
            .clone()
            .platform()
            .await?;

        alarms_generator::AlarmsGenerator::new_time_alarms(
            alarms_generator::Configuration {
                node_client: service_configuration.node_client().clone(),
                transaction_tx: transaction_tx.clone(),
                sender: service_configuration.signer().address().into(),
                address: time_alarms.check().await?.0.address().into(),
                alarms_per_message: task_creation_context
                    .time_alarms_per_message,
                gas_per_alarm: task_creation_context.gas_per_time_alarm,
                idle_duration: service_configuration.idle_duration(),
                timeout_duration: service_configuration.timeout_duration(),
            },
            TimeAlarms {},
        )
        .map(Task::TimeAlarms)
    }

    async fn create_price_alarms_task(
        service_configuration: &configuration::Service,
        task_creation_context: &ApplicationDefinedContext,
        transaction_tx: &channel::unbounded::Sender<TxPackage<NoExpiration>>,
        protocol_name: Arc<str>,
    ) -> Result<Task> {
        let GeneralizedProtocol {
            contracts: GeneralizedProtocolContracts { oracle, .. },
            network: _,
        } = service_configuration
            .admin_contract()
            .clone()
            .generalized_protocol(&protocol_name)
            .await?;

        alarms_generator::AlarmsGenerator::new_price_alarms(
            alarms_generator::Configuration {
                node_client: service_configuration.node_client().clone(),
                transaction_tx: transaction_tx.clone(),
                sender: service_configuration.signer().address().into(),
                address: oracle.check().await?.0.address().into(),
                alarms_per_message: task_creation_context
                    .price_alarms_per_message,
                gas_per_alarm: task_creation_context.gas_per_price_alarm,
                idle_duration: service_configuration.idle_duration(),
                timeout_duration: service_configuration.timeout_duration(),
            },
            PriceAlarms::new(protocol_name),
        )
        .map(Task::PriceAlarms)
    }
}

impl application_defined::Id for Id {
    type ServiceConfiguration = configuration::Service;

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

    async fn into_task<'r>(
        self,
        &mut ref service_configuration: &'r mut Self::ServiceConfiguration,
        &mut ref task_creation_context: &'r mut Self::TaskCreationContext,
        transaction_tx: &'r channel::unbounded::Sender<TxPackage<NoExpiration>>,
    ) -> Result<Task> {
        match self {
            Id::TimeAlarmsGenerator => {
                log!(info!("Creating time alarms generator."));

                Self::create_time_alarms_task(
                    service_configuration,
                    task_creation_context,
                    transaction_tx,
                )
                .await
            },
            Id::PriceAlarmsGenerator {
                protocol: protocol_name,
            } => {
                log!(info!(
                    protocol = %protocol_name,
                    "Creating price alarms generator for protocol.",
                ));

                Self::create_price_alarms_task(
                    service_configuration,
                    task_creation_context,
                    transaction_tx,
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
    async fn run(self, is_retry: RunnableState) -> Result<()> {
        match self {
            Task::TimeAlarms(alarms_generator) => {
                alarms_generator.run(is_retry).await
            },
            Task::PriceAlarms(alarms_generator) => {
                alarms_generator.run(is_retry).await
            },
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
