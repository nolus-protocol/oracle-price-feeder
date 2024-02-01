use std::future::Future;

use chain_comms::{client::Client as NodeClient, interact::commit, signer::Signer};

use crate::cache;

pub trait Impl: Send + Sync + Sized {
    type Expiration: Send + Sync + Sized;

    fn purge_cache(cache: &mut cache::TxRequests<Self>) -> PurgeResult;

    fn filter(expiration: &Self::Expiration) -> FilterResult;

    fn broadcast_commit(
        node_client: &NodeClient,
        signer: &mut Signer,
        signed_tx_bytes: Vec<u8>,
    ) -> impl Future<Output = Result<commit::Response, Vec<u8>>>;
}

pub enum PurgeResult {
    NotExhausted,
    Exhausted,
}

pub enum FilterResult {
    NotExpired,
    Expired,
}
