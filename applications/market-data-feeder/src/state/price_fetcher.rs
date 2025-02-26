use contract::{Admin, CheckedContract};
use std::sync::Arc;
use tokio::sync::Mutex;
use std::collections::BTreeMap;
use chain_ops::node;
use std::time::Duration;
use cosmrs::Gas;
use chain_ops::node::QueryTx;

#[derive(Clone)]
#[must_use]
pub struct State {
    pub admin_contract: CheckedContract<Admin>,
    pub dex_node_clients: Arc<Mutex<BTreeMap<Box<str>, node::Client>>>,
    pub idle_duration: Duration,
    pub signer_address: Arc<str>,
    pub hard_gas_limit: Gas,
    pub query_tx: QueryTx,
    pub timeout_duration: Duration,
}