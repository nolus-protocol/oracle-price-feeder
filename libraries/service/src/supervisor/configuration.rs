use std::time::Duration;

use anyhow::{Context as _, Result};
use zeroize::Zeroizing;

use chain_ops::{
    key, node,
    signer::{GasAndFeeConfiguration, Signer},
};
use contract::{Address, Admin, CheckedContract, UncheckedContract};
use environment::ReadFromVar as _;

#[must_use]
pub struct Service {
    pub node_client: node::Client,
    pub signer: Signer,
    pub admin_contract: CheckedContract<Admin>,
    pub idle_duration: Duration,
    pub timeout_duration: Duration,
}

impl Service {
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

        let (admin_contract, _) = UncheckedContract::admin(
            node_client.clone().query_wasm(),
            Address::new(Self::read_admin_contract_address()?),
        )
        .check()
        .await
        .context("Failed to connect to admin contract!")?;

        let idle_duration = Self::read_idle_duration()?;

        let timeout_duration = Self::read_timeout_duration()?;

        Ok(Self {
            node_client,
            signer,
            admin_contract,
            idle_duration,
            timeout_duration,
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
}
