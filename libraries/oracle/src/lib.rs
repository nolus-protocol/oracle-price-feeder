use anyhow::{Context as _, Result};

use chain_ops::node::Reconnect;
use contract::{CheckedContract, UncheckedContract};
use dex::Dex;
use semver::SemVer;

#[must_use]
pub struct Oracle<Dex>
where
    Dex: ?Sized,
{
    contract: CheckedContract<contract::Oracle<Dex>>,
    version: SemVer,
}

impl<Dex> Oracle<Dex>
where
    Dex: self::Dex + ?Sized,
{
    pub async fn new(
        contract: UncheckedContract<contract::Oracle<Dex>>,
    ) -> Result<Self> {
        contract
            .check()
            .await
            .context("Failed to check oracle contract's version!")
            .map(|(contract, version)| Self { contract, version })
    }
}

impl<Dex> Oracle<Dex> {
    #[inline]
    pub const fn contract(&self) -> &CheckedContract<contract::Oracle<Dex>> {
        &self.contract
    }

    #[inline]
    pub const fn contract_mut(
        &mut self,
    ) -> &mut CheckedContract<contract::Oracle<Dex>> {
        &mut self.contract
    }

    #[inline]
    #[must_use]
    pub fn address(&self) -> &str {
        self.contract.address()
    }

    #[inline]
    pub const fn version(&self) -> SemVer {
        self.version
    }
}

impl<Dex> Reconnect for Oracle<Dex>
where
    Dex: ?Sized,
{
    fn reconnect(&self) -> impl Future<Output = Result<()>> + Send + '_ {
        self.contract.reconnect()
    }
}
