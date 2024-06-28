use std::{
    collections::BTreeMap,
    sync::{OnceLock, RwLock, RwLockWriteGuard},
};

use serde::{de::DeserializeOwned, Deserialize, Serialize};

use chain_comms::{client::Client as NodeClient, interact::query};

use self::result::Result;

pub mod error;
pub mod result;

pub struct Platform;

impl Platform {
    pub async fn fetch(
        node: &NodeClient,
        admin: String,
    ) -> Result<PlatformContracts> {
        query_contract(
            node,
            admin,
            AdminContractQueryMsg::platform_query_message()?,
        )
        .await
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct PlatformContracts {
    #[serde(alias = "timealarms")]
    pub time_alarms: Box<str>,
}

#[derive(Deserialize)]
#[serde(transparent)]
pub struct Protocols(pub Box<[Protocol]>);

impl Protocols {
    pub async fn fetch(node: &NodeClient, admin: String) -> Result<Self> {
        query_contract(
            node,
            admin,
            AdminContractQueryMsg::protocols_query_message()?,
        )
        .await
        .map(Self)
    }
}

#[derive(Deserialize)]
#[serde(transparent)]
pub struct Protocol(Box<str>);

impl Protocol {
    pub async fn fetch(
        self,
        node: &NodeClient,
        admin: String,
    ) -> Result<ProtocolDefinition> {
        query_contract(
            node,
            admin,
            AdminContractQueryMsg::protocol_query_message(self.0)?,
        )
        .await
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ProtocolDefinition {
    pub network: Box<str>,
    pub contracts: ProtocolContracts,
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ProtocolContracts {
    pub oracle: Box<str>,
}

async fn query_contract<T>(
    node: &NodeClient,
    admin: String,
    query_data: Vec<u8>,
) -> Result<T>
where
    T: DeserializeOwned,
{
    query::wasm_smart(&mut node.wasm_query_client(), admin, query_data)
        .await
        .map_err(error::Error::QueryWasm)
}

#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
enum AdminContractQueryMsg<'r> {
    Platform {},
    Protocols {},
    Protocol(&'r str),
}

impl<'r> AdminContractQueryMsg<'r> {
    #[inline]
    fn platform_query_message() -> Result<Vec<u8>> {
        static QUERY_MESSAGE: OnceLock<Box<[u8]>> = OnceLock::new();

        Self::get_or_init(&QUERY_MESSAGE, &AdminContractQueryMsg::Platform {})
    }

    #[inline]
    fn protocols_query_message() -> Result<Vec<u8>> {
        static QUERY_MESSAGE: OnceLock<Box<[u8]>> = OnceLock::new();

        Self::get_or_init(&QUERY_MESSAGE, &AdminContractQueryMsg::Protocols {})
    }

    fn get_or_init(
        memoized: &OnceLock<Box<[u8]>>,
        message: &AdminContractQueryMsg<'_>,
    ) -> Result<Vec<u8>> {
        if let Some(query_message) = memoized.get() {
            Ok(query_message.to_vec())
        } else {
            serde_json_wasm::to_vec(&message)
                .map_err(error::Error::SerializeQueryMsg)
                .map(|serialized| {
                    // In case of a data-race condition, nothing useful can be
                    // done as the serialized message would already have been
                    // constructed.
                    let _ = memoized.set(serialized.clone().into_boxed_slice());

                    serialized
                })
        }
    }

    fn protocol_query_message(protocol: Box<str>) -> Result<Vec<u8>> {
        static QUERY_MESSAGES: RwLock<ProtocolQueryMessagesMap> =
            RwLock::new(ProtocolQueryMessagesMap::new());

        // Weak locking optimistic route.
        if let Ok(messages) = QUERY_MESSAGES.read() {
            if let Some(query_message) = messages.get(&protocol) {
                Ok(query_message.clone().into_vec())
            } else {
                drop(messages);

                // Strong locking optimistic route.
                // Handles race conditions between unlocking and locking.
                if let Ok(mut messages) = QUERY_MESSAGES.write() {
                    let query_message =
                        if let Some(query_message) = messages.get(&protocol) {
                            Ok(query_message.clone().into_vec())
                        } else {
                            Self::insert_protocol_query_message(
                                &mut messages,
                                protocol,
                            )
                        };

                    query_message
                } else {
                    unreachable!("Protocol query messages RwLock poisoned!");
                }
            }
        } else {
            unreachable!("Protocol query messages RwLock poisoned!");
        }
    }

    fn insert_protocol_query_message(
        messages: &mut RwLockWriteGuard<'_, ProtocolQueryMessagesMap>,
        protocol: Box<str>,
    ) -> Result<Vec<u8>> {
        let query_message = serde_json_wasm::to_vec(
            &AdminContractQueryMsg::Protocol(&protocol),
        )
        .map_err(error::Error::SerializeQueryMsg)?;

        if messages
            .insert(protocol, query_message.clone().into_boxed_slice())
            .is_some()
        {
            unreachable!(
                "Message already set but wasn't found in optimistic routes!"
            );
        }

        Ok(query_message)
    }
}

type ProtocolQueryMessagesMap = BTreeMap<Box<str>, Box<[u8]>>;
