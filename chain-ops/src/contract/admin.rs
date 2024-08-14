use std::sync::Arc;

use anyhow::Result;
use serde::{de::DeserializeOwned, Deserialize, Serialize};

use crate::node::QueryWasm;

#[derive(Clone)]
#[must_use]
pub struct Admin {
    query_wasm: QueryWasm,
    address: Arc<str>,
}

impl Admin {
    pub const fn new(query_wasm: QueryWasm, address: Arc<str>) -> Self {
        Self {
            query_wasm,
            address,
        }
    }

    pub async fn platform(&mut self) -> Result<Platform> {
        const QUERY_MSG: &[u8; 15] = br#"{"platform":{}}"#;

        self.query_wasm
            .smart(self.address.to_string(), QUERY_MSG.to_vec())
            .await
    }

    pub async fn protocols(&mut self) -> Result<Vec<String>> {
        const QUERY_MSG: &[u8; 16] = br#"{"protocols":{}}"#;

        self.query_wasm
            .smart(self.address.to_string(), QUERY_MSG.to_vec())
            .await
    }

    #[inline]
    pub async fn base_protocol(&mut self, name: &str) -> Result<BaseProtocol> {
        self.protocol_internal(name).await
    }

    #[inline]
    pub async fn protocol(&mut self, name: &str) -> Result<Protocol> {
        self.protocol_internal(name).await
    }

    async fn protocol_internal<T>(&mut self, name: &str) -> Result<T>
    where
        T: DeserializeOwned,
    {
        #[derive(Serialize)]
        #[serde(rename_all = "snake_case", deny_unknown_fields)]
        enum QueryMsg<'r> {
            Protocol(&'r str),
        }

        self.query_wasm
            .smart(
                self.address.to_string(),
                serde_json_wasm::to_vec(&QueryMsg::Protocol(name))?,
            )
            .await
            .map_err(Into::into)
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct Platform {
    #[serde(rename = "timealarms")]
    pub time_alarms: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct BaseProtocol {
    pub contracts: ProtocolContracts,
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct Protocol {
    pub network: String,
    pub dex: Dex,
    pub contracts: ProtocolContracts,
}

#[derive(Deserialize)]
#[serde(
    rename_all = "PascalCase",
    rename_all_fields = "snake_case",
    deny_unknown_fields
)]
pub enum Dex {
    Astroport { router_address: String },
    Osmosis,
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ProtocolContracts {
    pub oracle: String,
}
