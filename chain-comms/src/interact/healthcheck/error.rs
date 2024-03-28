use thiserror::Error as ThisError;

use crate::interact::query;

#[derive(Debug, ThisError)]
pub enum CheckSyncing {
    #[error("Error occurred while querying syncing status! Cause: {0}")]
    QuerySyncing(#[from] query::error::Syncing),
    #[error("Node is currently syncing!")]
    Syncing,
}

#[derive(Debug, ThisError)]
pub enum LatestBlockHeight {
    #[error("Error occurred while querying latest block! Cause: {0}")]
    LatestBlock(#[from] query::error::LatestBlock),
    #[error("Node didn't return block header information!")]
    NoBlockHeaderReturned,
    #[error("Node returned invalid block height! Error: {0}")]
    InvalidBlockHeightReturned(#[from] cosmrs::tendermint::Error),
}

#[derive(Debug, ThisError)]
pub enum Construct {
    #[error("Error occurred while checking syncing status! Cause: {0}")]
    Syncing(#[from] CheckSyncing),
    #[error("Error occurred while fetching latest block height! Cause: {0}")]
    LatestBlockHeight(#[from] LatestBlockHeight),
}

#[derive(Debug, ThisError)]
pub enum Error {
    #[error("Error occurred while checking syncing status! Cause: {0}")]
    Syncing(#[from] CheckSyncing),
    #[error("Error occurred while fetching latest block height! Cause: {0}")]
    LatestBlockHeight(#[from] LatestBlockHeight),
    #[error("Node returned decremented or equal block height!")]
    BlockHeightNotIncremented,
}
