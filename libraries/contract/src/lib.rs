use std::{convert::Infallible, marker::PhantomData, sync::Arc};

use anyhow::{Context as _, Result, anyhow};
use serde::{Deserialize, Serialize};

use chain_ops::node::{QueryWasm, Reconnect};
use dex::{
    Currencies, CurrencyPair, CurrencyPairs, Dex,
    providers::{astroport::Astroport, osmosis::Osmosis},
};
use semver::{Compatibility, SemVer};

pub trait Contract {
    const NAME: &'static str;

    const MINIMUM_COMPATIBLE_VERSION: SemVer;

    fn query_package_info(
        query_wasm: &mut QueryWasm,
        address: String,
    ) -> impl Future<Output = Result<PackageInfo>> + Send + '_;

    fn check_compatibility(package_info: &PackageInfo) -> Result<()> {
        if *package_info.name == *Self::NAME {
            match package_info
                .version
                .check_compatibility(Self::MINIMUM_COMPATIBLE_VERSION)
            {
                Compatibility::Compatible => Ok(()),
                Compatibility::Incompatible => Err(anyhow!(
                    "{contract:?} contract has an incompatible version! \
                    Minimum compatible version: {compatible_version}. \
                    Version: {version}.",
                    contract = Self::NAME,
                    compatible_version = Self::MINIMUM_COMPATIBLE_VERSION,
                    version = package_info.version,
                )),
            }
        } else {
            Err(anyhow!(
                "Contract did not match expected name! \
                Expected name: {expected:?}, but got: {actual:?}",
                expected = Self::NAME,
                actual = &*package_info.name,
            ))
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct PackageInfo {
    name: Box<str>,
    version: SemVer,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
struct SoftwareRelease {
    code: PackageInfo,
}

type PlatformVersion = SoftwareRelease;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
struct ProtocolVersion {
    software: SoftwareRelease,
}

async fn platform_package(
    query_wasm: &mut QueryWasm,
    address: String,
) -> Result<PackageInfo> {
    query_wasm
        .smart(address, br#"{"platform_package_release":{}}"#.to_vec())
        .await
        .map(|PlatformVersion { code }| code)
}

async fn protocol_package(
    query_wasm: &mut QueryWasm,
    address: String,
) -> Result<PackageInfo> {
    query_wasm
        .smart(address, br#"{"protocol_package_release":{}}"#.to_vec())
        .await
        .map(
            |ProtocolVersion {
                 software: SoftwareRelease { code },
             }| code,
        )
}

#[must_use]
pub struct UncheckedContract<Contract>
where
    Contract: ?Sized,
{
    query_wasm: QueryWasm,
    address: Address,
    _contract: PhantomData<Contract>,
}

impl<Contract> UncheckedContract<Contract>
where
    Contract: ?Sized,
{
    #[inline]
    const fn new(query_wasm: QueryWasm, address: Address) -> Self {
        Self {
            query_wasm,
            address,
            _contract: PhantomData,
        }
    }
}

impl<Contract> UncheckedContract<Contract>
where
    Contract: self::Contract + ?Sized,
{
    pub async fn check(
        mut self,
    ) -> Result<(CheckedContract<Contract>, SemVer)> {
        Contract::query_package_info(
            &mut self.query_wasm,
            self.address.0.clone(),
        )
        .await
        .with_context(|| {
            format!(
                "Failed to query {contract:?} contract's package information!",
                contract = Contract::NAME,
            )
        })
        .and_then(|package_info| {
            Contract::check_compatibility(&package_info)
                .map(|()| {
                    let Self {
                        query_wasm,
                        address,
                        _contract: PhantomData {},
                    } = self;

                    (
                        CheckedContract {
                            query_wasm,
                            address,
                            _contract: PhantomData,
                        },
                        package_info.version,
                    )
                })
                .with_context(|| {
                    format!(
                        "Version compatibility check failed for {contract:?} contract!",
                        contract = Contract::NAME,
                    )
                })
        })
    }
}

impl UncheckedContract<Admin> {
    #[inline]
    pub const fn admin(query_wasm: QueryWasm, address: Address) -> Self {
        Self::new(query_wasm, address)
    }
}

#[must_use]
pub struct CheckedContract<Contract>
where
    Contract: ?Sized,
{
    query_wasm: QueryWasm,
    address: Address,
    _contract: PhantomData<Contract>,
}

impl<Contract> CheckedContract<Contract>
where
    Contract: ?Sized,
{
    #[inline]
    pub const fn query_wasm(&self) -> &QueryWasm {
        &self.query_wasm
    }

    #[inline]
    pub const fn query_wasm_mut(&mut self) -> &mut QueryWasm {
        &mut self.query_wasm
    }

    #[inline]
    #[must_use]
    pub fn address(&self) -> &str {
        &self.address.0
    }
}

impl<Contract> CheckedContract<Contract>
where
    Contract: self::Contract + ?Sized,
{
    pub async fn version(&mut self) -> Result<SemVer> {
        Contract::query_package_info(
            &mut self.query_wasm,
            self.address.0.clone(),
        )
        .await
        .map(|PackageInfo { version, .. }| version)
        .with_context(|| {
            format!(
                "Failed to query {contract:?} contract's version!",
                contract = Contract::NAME,
            )
        })
    }

    pub async fn recheck_version(&mut self) -> Result<()> {
        Contract::query_package_info(
            &mut self.query_wasm,
            self.address.0.clone(),
        )
        .await
        .with_context(|| {
            format!(
                "Failed to query {contract:?} contract's package information!",
                contract = Contract::NAME,
            )
        })
        .and_then(|package_info| {
            Contract::check_compatibility(&package_info).with_context(|| {
                format!(
                    "Version compatibility check failed for {contract:?} contract!",
                    contract = Contract::NAME,
                )
            })
        })
    }
}

impl CheckedContract<Admin> {
    pub async fn platform(&mut self) -> Result<Platform> {
        #[derive(Debug, Deserialize)]
        #[serde(rename_all = "snake_case")]
        pub struct Platform {
            #[serde(rename = "timealarms")]
            pub time_alarms: Address,
        }

        const QUERY_MSG: &[u8; 15] = br#"{"platform":{}}"#;

        self.query_wasm
            .smart(self.address.0.clone(), QUERY_MSG.to_vec())
            .await
            .map(|Platform { time_alarms }| self::Platform {
                time_alarms: UncheckedContract::new(
                    self.query_wasm.clone(),
                    time_alarms,
                ),
            })
    }

    pub async fn protocols(&mut self) -> Result<Box<[Arc<str>]>> {
        const QUERY_MSG: &[u8; 16] = br#"{"protocols":{}}"#;

        self.query_wasm
            .smart(self.address.0.clone(), QUERY_MSG.to_vec())
            .await
            .context("Failed to fetch protocols' names!")
    }

    pub async fn generalized_protocol(
        &mut self,
        name: &str,
    ) -> Result<GeneralizedProtocol> {
        #[derive(Serialize)]
        #[serde(rename_all = "snake_case")]
        enum QueryMsg<'r> {
            Protocol(&'r str),
        }

        #[derive(Deserialize)]
        #[serde(rename_all = "snake_case")]
        struct Contracts {
            oracle: Address,
        }

        #[derive(Deserialize)]
        #[serde(rename_all = "snake_case")]
        struct Protocol {
            contracts: Contracts,
            network: String,
        }

        self.query_wasm
            .smart(
                self.address.0.clone(),
                serde_json_wasm::to_vec(&QueryMsg::Protocol(name))?,
            )
            .await
            .map(
                |Protocol {
                     contracts: Contracts { oracle },
                     network,
                 }| GeneralizedProtocol {
                    contracts: GeneralizedProtocolContracts {
                        oracle: UncheckedContract::new(
                            self.query_wasm.clone(),
                            oracle,
                        ),
                    },
                    network,
                },
            )
            .context("Failed to fetch protocol, in generalized form!")
    }

    pub async fn protocol(&mut self, name: &str) -> Result<Protocol> {
        #[derive(Serialize)]
        #[serde(rename_all = "snake_case")]
        enum QueryMsg<'r> {
            Protocol(&'r str),
        }

        #[derive(Deserialize)]
        #[serde(rename_all = "PascalCase", rename_all_fields = "snake_case")]
        pub enum Dex {
            Astroport { router_address: String },
            Osmosis,
        }

        #[derive(Deserialize)]
        #[serde(rename_all = "snake_case")]
        struct Contracts {
            oracle: Address,
        }

        #[derive(Deserialize)]
        #[serde(rename_all = "snake_case")]
        struct Protocol {
            contracts: Contracts,
            dex: Dex,
            network: String,
        }

        fn construct_provider_and_contracts<Dex>(
            query_wasm: &QueryWasm,
            provider: Dex,
            Contracts { oracle }: Contracts,
        ) -> ProtocolProviderAndContracts<Dex> {
            ProtocolProviderAndContracts {
                provider,
                oracle: UncheckedContract::new(query_wasm.clone(), oracle),
            }
        }

        self.query_wasm
            .smart(
                self.address.0.clone(),
                serde_json_wasm::to_vec(&QueryMsg::Protocol(name))?,
            )
            .await
            .map(
                move |Protocol {
                          contracts,
                          dex,
                          network,
                      }| self::Protocol {
                    network,
                    provider_and_contracts: match dex {
                        Dex::Astroport { router_address } => {
                            ProtocolDex::Astroport(
                                construct_provider_and_contracts(
                                    &self.query_wasm,
                                    Astroport::new(router_address),
                                    contracts,
                                ),
                            )
                        },
                        Dex::Osmosis => ProtocolDex::Osmosis(
                            construct_provider_and_contracts(
                                &self.query_wasm,
                                Osmosis::new(),
                                contracts,
                            ),
                        ),
                    },
                },
            )
            .context("Failed to fetch protocol, in specialized form!")
    }
}

impl<Dex> CheckedContract<Oracle<Dex>>
where
    Dex: self::Dex + ?Sized,
{
    pub async fn query_currencies(&mut self) -> Result<Currencies> {
        #[derive(Deserialize)]
        #[serde(rename_all = "snake_case")]
        struct Currency {
            ticker: String,
            dex_symbol: String,
            decimal_digits: u8,
        }

        const QUERY_MESSAGE: &[u8; 17] = br#"{"currencies":{}}"#;

        self.query_wasm
            .smart(self.address.0.clone(), QUERY_MESSAGE.to_vec())
            .await
            .map(|currencies: Vec<_>| {
                currencies
                    .into_iter()
                    .map(
                        |Currency {
                             ticker,
                             dex_symbol,
                             decimal_digits,
                         }| {
                            (
                                ticker,
                                dex::Currency {
                                    dex_symbol,
                                    decimal_digits,
                                },
                            )
                        },
                    )
                    .collect()
            })
            .context("Failed to query for oracle contract currencies!")
    }

    pub async fn query_currency_pairs(&mut self) -> Result<CurrencyPairs<Dex>> {
        const QUERY_MESSAGE: &[u8; 31] = br#"{"supported_currency_pairs":{}}"#;

        self.query_wasm
            .smart(self.address.0.clone(), QUERY_MESSAGE.to_vec())
            .await
            .map(|currency_pairs: Vec<_>| {
                currency_pairs
                    .into_iter()
                    .map(|(base, (pool_id, quote))| {
                        (CurrencyPair { base, quote }, pool_id)
                    })
                    .collect()
            })
            .context(
                "Failed to query for oracle contract's configured currency \
                pairs!",
            )
    }
}

impl<Contract> Clone for CheckedContract<Contract>
where
    Contract: ?Sized,
{
    fn clone(&self) -> Self {
        Self {
            query_wasm: self.query_wasm.clone(),
            address: self.address.clone(),
            _contract: PhantomData,
        }
    }
}

impl<Contract> Reconnect for CheckedContract<Contract>
where
    Contract: ?Sized,
{
    fn reconnect(&self) -> impl Future<Output = Result<()>> + Send + '_ {
        self.query_wasm.reconnect()
    }
}

#[derive(Debug, Clone, Deserialize)]
#[repr(transparent)]
#[serde(transparent)]
pub struct Address(String);

impl Address {
    #[inline]
    #[must_use]
    pub const fn new(address: String) -> Self {
        Self(address)
    }
}

pub struct Platform {
    pub time_alarms: UncheckedContract<TimeAlarms>,
}

pub struct GeneralizedProtocol {
    pub contracts: GeneralizedProtocolContracts,
    pub network: String,
}

pub struct GeneralizedProtocolContracts {
    pub oracle: UncheckedContract<GeneralizedOracle>,
}

pub struct Protocol {
    pub network: String,
    pub provider_and_contracts: ProtocolDex,
}

pub enum ProtocolDex {
    Astroport(ProtocolProviderAndContracts<Astroport>),
    Osmosis(ProtocolProviderAndContracts<Osmosis>),
}

pub struct ProtocolProviderAndContracts<Dex> {
    pub provider: Dex,
    pub oracle: UncheckedContract<Oracle<Dex>>,
}

pub enum Admin {}

impl Contract for Admin {
    const NAME: &'static str = "admin_contract";

    const MINIMUM_COMPATIBLE_VERSION: SemVer = SemVer::new(0, 6, 0);

    #[inline]
    fn query_package_info(
        query_wasm: &mut QueryWasm,
        address: String,
    ) -> impl Future<Output = Result<PackageInfo>> + Send + '_ {
        platform_package(query_wasm, address)
    }
}

pub enum GeneralizedOracle {}

impl Contract for GeneralizedOracle {
    const NAME: &'static str = "oracle";

    const MINIMUM_COMPATIBLE_VERSION: SemVer = SemVer::new(0, 6, 0);

    #[inline]
    fn query_package_info(
        query_wasm: &mut QueryWasm,
        address: String,
    ) -> impl Future<Output = Result<PackageInfo>> + Send + '_ {
        protocol_package(query_wasm, address)
    }
}

pub struct Oracle<Dex>
where
    Dex: ?Sized,
{
    _infallible: Infallible,
    _dex: PhantomData<Dex>,
}

impl<Dex> Contract for Oracle<Dex>
where
    Dex: ?Sized,
{
    const NAME: &'static str = GeneralizedOracle::NAME;

    const MINIMUM_COMPATIBLE_VERSION: SemVer =
        GeneralizedOracle::MINIMUM_COMPATIBLE_VERSION;

    #[inline]
    fn query_package_info(
        query_wasm: &mut QueryWasm,
        address: String,
    ) -> impl Future<Output = Result<PackageInfo>> + Send + '_ {
        GeneralizedOracle::query_package_info(query_wasm, address)
    }
}

pub enum TimeAlarms {}

impl Contract for TimeAlarms {
    const NAME: &'static str = "timealarms";

    const MINIMUM_COMPATIBLE_VERSION: SemVer = SemVer::new(0, 5, 0);

    #[inline]
    fn query_package_info(
        query_wasm: &mut QueryWasm,
        address: String,
    ) -> impl Future<Output = Result<PackageInfo>> + Send + '_ {
        platform_package(query_wasm, address)
    }
}
