use std::{collections::BTreeMap, sync::Arc};

use anyhow::Result;
use tokio::sync::Mutex;

use channel::{bounded, unbounded};
use protocol_watcher::Command;
use service::supervisor::configuration::Service;
use tx::{TimeBasedExpiration, TxPackage};

use crate::PriceFetcherState;

pub(crate) struct State {
    balance_reporter: balance_reporter::State,
    broadcaster: broadcaster::State<TimeBasedExpiration>,
    protocol_watcher: protocol_watcher::State,
    price_fetcher: PriceFetcherState,
}

impl State {
    pub fn new(
        service: Service,
        transaction_rx: unbounded::Receiver<TxPackage<TimeBasedExpiration>>,
        action_tx: bounded::Sender<Command>,
    ) -> Result<State> {
        let signer_address: Arc<str> = service.signer.address().into();

        let balance_reporter = {
            let balance_reporter::Environment { idle_duration } =
                balance_reporter::Environment::read_from_env()?;

            balance_reporter::State {
                query_bank: service.node_client.clone().query_bank(),
                address: signer_address.clone(),
                denom: service.signer.fee_token().into(),
                idle_duration,
            }
        };

        let broadcaster = {
            let broadcaster::Environment {
                delay_duration,
                retry_delay_duration,
            } = broadcaster::Environment::read_from_env()?;

            broadcaster::State {
                broadcast_tx: service.node_client.clone().broadcast_tx(),
                signer: Arc::new(Mutex::new(service.signer)),
                transaction_rx: Arc::new(Mutex::new(transaction_rx)),
                delay_duration,
                retry_delay_duration,
            }
        };

        let protocol_watcher = protocol_watcher::State {
            admin_contract: service.admin_contract.clone(),
            action_tx,
        };

        let price_fetcher = PriceFetcherState {
            admin_contract: service.admin_contract,
            dex_node_clients: Arc::new(Mutex::new(BTreeMap::new())),
            idle_duration: service.idle_duration,
            signer_address,
            hard_gas_limit: 0,
            query_tx: service.node_client.clone().query_tx(),
            timeout_duration: service.idle_duration,
        };

        Ok(State {
            balance_reporter,
            broadcaster,
            protocol_watcher,
            price_fetcher,
        })
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
    pub const fn price_fetcher(&self) -> &PriceFetcherState {
        &self.price_fetcher
    }
}
