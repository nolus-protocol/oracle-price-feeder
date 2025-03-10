use std::{collections::BTreeMap, sync::Arc, time::Duration};

use anyhow::Result;
use tokio::sync::Mutex;

use chain_ops::{
    node::{self, QueryTx},
    signer::{Gas, Signer},
};
use channel::{bounded, unbounded};
use contract::{Admin, CheckedContract};
use protocol_watcher::Command;
use service::supervisor::configuration::Service;
use tx::{TimeBasedExpiration, TxPackage};

pub struct State {
    error_handler: error_handler::State,
    balance_reporter: balance_reporter::State,
    broadcaster: broadcaster::State<TimeBasedExpiration>,
    protocol_watcher: protocol_watcher::State,
    price_fetcher: PriceFetcher,
}

impl State {
    pub fn new(
        Service {
            node_client,
            signer,
            admin_contract,
            idle_duration,
            timeout_duration,
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

        let protocol_watcher = protocol_watcher::State {
            admin_contract: admin_contract.clone(),
            action_tx,
        };

        let price_fetcher = Self::new_price_fetcher(
            node_client,
            admin_contract,
            idle_duration,
            timeout_duration,
            signer_address,
        );

        Ok(State {
            error_handler,
            balance_reporter,
            broadcaster,
            protocol_watcher,
            price_fetcher,
        })
    }

    #[inline]
    pub const fn error_handler(&self) -> &error_handler::State {
        &self.error_handler
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
    pub const fn price_fetcher(&self) -> &PriceFetcher {
        &self.price_fetcher
    }

    fn new_error_handler() -> Result<error_handler::State> {
        use error_handler::{Environment, State};

        Environment::read_from_env().map(
            |Environment {
                 non_delayed_task_retries_count,
                 failed_retry_margin,
             }| State {
                non_delayed_task_retries_count,
                failed_retry_margin,
            },
        )
    }

    fn new_balance_reporter(
        node_client: node::Client,
        address: Arc<str>,
        denom: Arc<str>,
    ) -> Result<balance_reporter::State> {
        use balance_reporter::{Environment, State};

        Environment::read_from_env().map(|Environment { idle_duration }| {
            State {
                query_bank: node_client.clone().query_bank(),
                address,
                denom,
                idle_duration,
            }
        })
    }

    fn new_broadcaster(
        node_client: node::Client,
        signer: Signer,
        transaction_rx: tokio::sync::mpsc::UnboundedReceiver<
            TxPackage<TimeBasedExpiration>,
        >,
    ) -> Result<broadcaster::State<TimeBasedExpiration>, anyhow::Error> {
        let broadcaster::Environment {
            delay_duration,
            retry_delay_duration,
        } = broadcaster::Environment::read_from_env()?;
        Ok(broadcaster::State {
            broadcast_tx: node_client.broadcast_tx(),
            signer: Arc::new(Mutex::new(signer)),
            transaction_rx: Arc::new(Mutex::new(transaction_rx)),
            delay_duration,
            retry_delay_duration,
        })
    }

    fn new_price_fetcher(
        node_client: node::Client,
        admin_contract: CheckedContract<Admin>,
        idle_duration: Duration,
        timeout_duration: Duration,
        signer_address: Arc<str>,
    ) -> PriceFetcher {
        PriceFetcher {
            admin_contract,
            dex_node_clients: Arc::new(Mutex::new(BTreeMap::new())),
            idle_duration,
            signer_address,
            hard_gas_limit: 0,
            query_tx: node_client.query_tx(),
            timeout_duration,
        }
    }
}

#[derive(Clone)]
#[must_use]
pub struct PriceFetcher {
    pub admin_contract: CheckedContract<Admin>,
    pub dex_node_clients: Arc<Mutex<BTreeMap<Box<str>, node::Client>>>,
    pub idle_duration: Duration,
    pub signer_address: Arc<str>,
    pub hard_gas_limit: Gas,
    pub query_tx: QueryTx,
    pub timeout_duration: Duration,
}
