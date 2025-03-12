use std::sync::Arc;

use anyhow::Result;
use tokio::sync::Mutex;

use chain_ops::{node, signer::Signer};
use channel::{bounded, unbounded};
use contract::{Admin, CheckedContract};
use protocol_watcher::Command;
use service::supervisor::configuration::Service;
use tx::{TimeBasedExpiration, TxPackage};

pub struct State {
    error_handler: error_handler::State<Arc<str>>,
    balance_reporter: balance_reporter::State,
    broadcaster: broadcaster::State<TimeBasedExpiration>,
    protocol_watcher: protocol_watcher::State,
    price_fetcher: price_fetcher::State,
}

impl State {
    pub fn new(
        Service {
            node_client,
            signer,
            admin_contract,
            idle_duration: _,
            timeout_duration: _,
        }: Service,
        transaction_rx: unbounded::Receiver<TxPackage<TimeBasedExpiration>>,
        action_tx: bounded::Sender<Command>,
    ) -> Result<State> {
        let signer_address: Arc<str> = signer.address().into();

        let error_handler = Self::new_error_handler()?;

        let balance_reporter = Self::new_balance_reporter(
            node_client.clone(),
            signer_address.clone(),
            signer.fee_token().into(),
        )?;

        let broadcaster =
            Self::new_broadcaster(node_client.clone(), signer, transaction_rx)?;

        let protocol_watcher =
            protocol_watcher::State::new(admin_contract.clone(), action_tx);

        let price_fetcher = Self::new_price_fetcher(
            node_client,
            admin_contract,
            signer_address,
        )?;

        Ok(State {
            error_handler,
            balance_reporter,
            broadcaster,
            protocol_watcher,
            price_fetcher,
        })
    }

    #[inline]
    pub const fn error_handler_mut(
        &mut self,
    ) -> &mut error_handler::State<Arc<str>> {
        &mut self.error_handler
    }

    #[inline]
    pub const fn balance_reporter(&self) -> &balance_reporter::State {
        &self.balance_reporter
    }

    #[inline]
    pub const fn broadcaster(
        &self,
    ) -> &broadcaster::State<TimeBasedExpiration> {
        &self.broadcaster
    }

    #[inline]
    pub const fn protocol_watcher(&self) -> &protocol_watcher::State {
        &self.protocol_watcher
    }

    #[inline]
    pub const fn price_fetcher(&self) -> &price_fetcher::State {
        &self.price_fetcher
    }

    fn new_error_handler() -> Result<error_handler::State<Arc<str>>> {
        use error_handler::{Environment, State};

        Environment::read_from_env().map(State::new)
    }

    fn new_balance_reporter(
        node_client: node::Client,
        address: Arc<str>,
        denom: Arc<str>,
    ) -> Result<balance_reporter::State> {
        use balance_reporter::{Environment, State};

        Environment::read_from_env().map(|environment| {
            State::new(
                environment,
                node_client.clone().query_bank(),
                address,
                denom,
            )
        })
    }

    fn new_broadcaster(
        node_client: node::Client,
        signer: Signer,
        transaction_rx: unbounded::Receiver<TxPackage<TimeBasedExpiration>>,
    ) -> Result<broadcaster::State<TimeBasedExpiration>> {
        use broadcaster::{Environment, State};

        Environment::read_from_env().map(|environment| {
            State::new(
                environment,
                node_client.broadcast_tx(),
                Arc::new(Mutex::new(signer)),
                Arc::new(Mutex::new(transaction_rx)),
            )
        })
    }

    fn new_price_fetcher(
        node_client: node::Client,
        admin_contract: CheckedContract<Admin>,
        signer_address: Arc<str>,
    ) -> Result<price_fetcher::State> {
        use self::price_fetcher::{Environment, State};

        Environment::read_from_env().map(|environment| {
            State::new(
                environment,
                admin_contract,
                signer_address,
                node_client.query_tx(),
            )
        })
    }
}

pub mod price_fetcher {
    use std::{collections::BTreeMap, sync::Arc, time::Duration};

    use anyhow::Result;
    use tokio::sync::Mutex;

    use chain_ops::{
        node::{self, QueryTx},
        signer::Gas,
    };
    use contract::{Admin, CheckedContract};
    use environment::ReadFromVar;

    pub struct Environment {
        duration_before_start: Duration,
        idle_duration: Duration,
        timeout_duration: Duration,
    }

    impl Environment {
        pub fn read_from_env() -> Result<Self> {
            let duration_before_start =
                ReadFromVar::read_from_var("DURATION_SECONDS_BEFORE_START")
                    .map(Duration::from_secs)?;

            let idle_duration =
                ReadFromVar::read_from_var("IDLE_DURATION_SECONDS")
                    .map(Duration::from_secs)?;

            let timeout_duration =
                ReadFromVar::read_from_var("TIMEOUT_DURATION_SECONDS")
                    .map(Duration::from_secs)?;

            Ok(Self {
                duration_before_start,
                idle_duration,
                timeout_duration,
            })
        }
    }

    #[derive(Clone)]
    #[must_use]
    pub struct State {
        pub admin_contract: CheckedContract<Admin>,
        pub dex_node_clients: Arc<Mutex<BTreeMap<Box<str>, node::Client>>>,
        pub duration_before_start: Duration,
        pub idle_duration: Duration,
        pub signer_address: Arc<str>,
        pub hard_gas_limit: Gas,
        pub query_tx: QueryTx,
        pub timeout_duration: Duration,
    }

    impl State {
        #[inline]
        pub fn new(
            Environment {
                duration_before_start,
                idle_duration,
                timeout_duration,
            }: Environment,
            admin_contract: CheckedContract<Admin>,
            signer_address: Arc<str>,
            query_tx: QueryTx,
        ) -> Self {
            Self {
                admin_contract,
                dex_node_clients: Arc::new(Mutex::new(BTreeMap::new())),
                duration_before_start,
                idle_duration,
                signer_address,
                hard_gas_limit: 0,
                query_tx,
                timeout_duration,
            }
        }
    }
}
