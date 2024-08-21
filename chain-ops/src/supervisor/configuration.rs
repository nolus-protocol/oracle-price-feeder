use std::{sync::Arc, time::Duration};

use anyhow::{Context as _, Error, Result};
use zeroize::Zeroizing;

use crate::{
    env::ReadFromVar,
    key, node,
    service::{task_spawner::TaskSpawner, TaskResultsReceiver},
    signer::{GasAndFeeConfiguration, Signer},
    task::{self, application_defined},
};

#[must_use]
pub struct Static {
    node_client: node::Client,
    signer: Signer,
    admin_contract_address: Arc<str>,
    idle_duration: Duration,
    timeout_duration: Duration,
    balance_reporter_idle_duration: Duration,
    broadcast_delay_duration: Duration,
    broadcast_retry_delay_duration: Duration,
}

impl Static {
    pub async fn read_from_env() -> Result<Self> {
        let node_client = node::Client::connect(&Self::read_node_grpc_uri()?)
            .await
            .context("Failed to connect to node's gRPC!")?;

        let signer = Signer::new(
            node_client.clone(),
            Self::derive_signing_key()?,
            Self::read_fee_token_denominator()?,
            Self::read_gas_and_fee_configuration()?,
        )
        .await?;

        let admin_contract_address =
            Self::read_admin_contract_address()?.into();

        let idle_duration = Self::read_idle_duration()?;

        let timeout_duration = Self::read_timeout_duration()?;

        let balance_reporter_idle_duration =
            Self::read_balance_reporter_idle_duration()?;

        let broadcast_delay_duration = Self::read_broadcast_delay_duration()?;

        let broadcast_retry_delay_duration =
            Self::read_broadcast_retry_delay_duration()?;

        Ok(Self {
            node_client,
            signer,
            admin_contract_address,
            idle_duration,
            timeout_duration,
            balance_reporter_idle_duration,
            broadcast_delay_duration,
            broadcast_retry_delay_duration,
        })
    }

    fn read_node_grpc_uri() -> Result<String> {
        String::read_from_var("NODE_GRPC_URI")
            .context("Failed to read node's gRPC URI!")
    }

    fn derive_signing_key() -> Result<key::Signing> {
        key::derive_from_mnemonic(&Self::read_signing_key_mnemonic()?, "")
            .context("Failed to derive signing key from mnemonic!")
    }

    fn read_signing_key_mnemonic() -> Result<Zeroizing<String>> {
        String::read_from_var("SIGNING_KEY_MNEMONIC")
            .context("Failed to read signing key's mnemonic!")
            .map(Zeroizing::new)
    }

    fn read_fee_token_denominator() -> Result<String> {
        String::read_from_var("FEE_TOKEN_DENOM")
            .context("Failed to read fee token's denominator!")
    }

    fn read_gas_and_fee_configuration() -> Result<GasAndFeeConfiguration> {
        GasAndFeeConfiguration::read_from_var("GAS_FEE_CONF")
            .context("Failed to read gas and fee configuration!")
    }

    fn read_admin_contract_address() -> Result<String> {
        String::read_from_var("ADMIN_CONTRACT_ADDRESS")
            .context("Failed to read admin contract's address")
    }

    fn read_idle_duration() -> Result<Duration> {
        u64::read_from_var("IDLE_DURATION_SECONDS")
            .map(Duration::from_secs)
            .context("Failed to read idle period duration!")
    }

    fn read_timeout_duration() -> Result<Duration> {
        u64::read_from_var("TIMEOUT_DURATION_SECONDS")
            .map(Duration::from_secs)
            .context("Failed to read timeout period duration!")
    }

    fn read_balance_reporter_idle_duration() -> Result<Duration, Error> {
        u64::read_from_var("BALANCE_REPORTER_IDLE_DURATION_SECONDS")
            .map(Duration::from_secs)
            .context("Failed to read between balance reporter idle delay period duration!")
    }

    fn read_broadcast_delay_duration() -> Result<Duration, Error> {
        u64::read_from_var("BROADCAST_DELAY_DURATION_SECONDS")
            .map(Duration::from_secs)
            .context("Failed to read between broadcast delay period duration!")
    }

    fn read_broadcast_retry_delay_duration() -> Result<Duration, Error> {
        u64::read_from_var("BROADCAST_RETRY_DELAY_DURATION_MILLISECONDS")
            .map(Duration::from_millis)
            .context("Failed to read between broadcast retries delay period duration!")
    }
}

#[must_use]
pub struct Configuration<T>
where
    T: application_defined::Task,
{
    pub(super) node_client: node::Client,
    pub(super) signer: Signer,
    pub(super) admin_contract_address: Arc<str>,
    pub(super) task_spawner: TaskSpawner<task::Id<T::Id>, Result<()>>,
    pub(super) task_result_rx: TaskResultsReceiver<task::Id<T::Id>, Result<()>>,
    pub(super) idle_duration: Duration,
    pub(super) timeout_duration: Duration,
    pub(super) balance_reporter_idle_duration: Duration,
    pub(super) broadcast_delay_duration: Duration,
    pub(super) broadcast_retry_delay_duration: Duration,
    pub(super) task_creation_context:
        application_defined::TaskCreationContext<T>,
}

impl<T> Configuration<T>
where
    T: application_defined::Task,
{
    pub fn new(
        Static {
            node_client,
            signer,
            admin_contract_address,
            idle_duration,
            timeout_duration,
            balance_reporter_idle_duration,
            broadcast_delay_duration,
            broadcast_retry_delay_duration,
        }: Static,
        task_spawner: TaskSpawner<task::Id<T::Id>, Result<()>>,
        task_result_rx: TaskResultsReceiver<task::Id<T::Id>, Result<()>>,
        task_creation_context:
        <T::Id as application_defined::Id>::TaskCreationContext,
    ) -> Self {
        Self {
            node_client,
            signer,
            admin_contract_address,
            task_spawner,
            task_result_rx,
            idle_duration,
            timeout_duration,
            balance_reporter_idle_duration,
            broadcast_delay_duration,
            broadcast_retry_delay_duration,
            task_creation_context,
        }
    }
}
