use serde_json_wasm::ser::Error as SerializationError;

use chain_comms::interact::query::error::Wasm;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Failed to query contract! Cause: {0}")]
    QueryWasm(#[from] Wasm),
    #[error("Failed to serialize query message! Cause: {0}")]
    SerializeQueryMsg(SerializationError),
}
