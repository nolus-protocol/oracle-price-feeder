use std::{sync::Arc, time::Duration};

use anyhow::{Context as _, Result};
use cosmrs::{
    proto::{
        cosmos::base::abci::v1beta1::TxResponse,
        cosmwasm::wasm::v1::MsgExecuteContract,
    },
    tendermint::{abci::Code as TxCode, block::Height},
    tx::Body as TxBody,
    Any, Gas,
};
use serde::{Deserialize, Serialize};
use tokio::{
    sync::{mpsc, oneshot},
    time::sleep,
};

use chain_ops::{node, tx};
use channel::unbounded;
use contract::{CheckedContract, GeneralizedOracle};
use service::task::{NoExpiration, Runnable, TxPackage};
use task::RunnableState;

macro_rules! log {
    ($macro:ident![$self:expr]($($body:tt)+)) => {
        ::tracing::$macro!(
            target: "alarms-dispatcher",
            source = %$self.source,
            $($body)+
        );
    };
}

macro_rules! log_with_hash {
    ($macro:ident![$self:expr, $response:expr]($($body:tt)+)) => {
        log!($macro![$self](
            hash = %$response.txhash,
            $($body)+
        ));
    };
}

pub struct Configuration {
    pub transaction_tx: unbounded::Sender<TxPackage<NoExpiration>>,
    pub sender: String,
    pub alarms_per_message: u32,
    pub gas_per_alarm: Gas,
    pub idle_duration: Duration,
    pub query_tx: node::QueryTx,
    pub timeout_duration: Duration,
}

pub trait Alarms: Send + Sized + 'static {
    const TYPE: &'static str;

    type Contract: Send + ?Sized;
}

#[derive(Clone)]
pub struct AlarmsGenerator<T>
where
    T: Alarms,
{
    contract: CheckedContract<T::Contract>,
    query_tx: node::QueryTx,
    transaction_tx: mpsc::UnboundedSender<TxPackage<NoExpiration>>,
    alarms_per_message: u32,
    gas_per_alarm: Gas,
    idle_duration: Duration,
    timeout_duration: Duration,
    tx_body: Arc<TxBody>,
    source: Arc<str>,
    alarms: T,
}

impl AlarmsGenerator<PriceAlarms> {
    #[inline]
    pub fn new_price_alarms(
        configuration: Configuration,
        contract: CheckedContract<<PriceAlarms as Alarms>::Contract>,
        alarms: PriceAlarms,
    ) -> Result<Self> {
        Self::with_source(
            configuration,
            format!("Price Alarms; Protocol={}", alarms.protocol()).into(),
            contract,
            alarms,
        )
    }
}

impl AlarmsGenerator<TimeAlarms> {
    #[inline]
    pub fn new_time_alarms(
        configuration: Configuration,
        contract: CheckedContract<<TimeAlarms as Alarms>::Contract>,
        alarms: TimeAlarms,
    ) -> Result<Self> {
        Self::with_source(configuration, "Time Alarms".into(), contract, alarms)
    }
}

impl<T> AlarmsGenerator<T>
where
    T: Alarms,
{
    pub fn with_source(
        Configuration {
            transaction_tx,
            sender,
            alarms_per_message,
            gas_per_alarm,
            idle_duration,
            query_tx,
            timeout_duration,
        }: Configuration,
        source: Arc<str>,
        contract: CheckedContract<T::Contract>,
        alarms: T,
    ) -> Result<Self> {
        Any::from_msg(&MsgExecuteContract {
            sender,
            contract: contract.address().to_string(),
            msg: format!(
                r#"{{"dispatch_alarms":{{"max_count":{alarms_per_message}}}}}"#,
            )
            .into_bytes(),
            funds: vec![],
        })
        .map(|message| Self {
            contract,
            query_tx,
            transaction_tx,
            alarms_per_message,
            gas_per_alarm,
            idle_duration,
            timeout_duration,
            tx_body: Arc::new(TxBody {
                messages: vec![message],
                memo: String::new(),
                timeout_height: Height::from(0_u8),
                extension_options: Vec::new(),
                non_critical_extension_options: Vec::new(),
            }),
            source,
            alarms,
        })
        .map_err(Into::into)
    }

    #[inline]
    pub(super) const fn alarms(&self) -> &T {
        &self.alarms
    }

    async fn dispatch_alarms(mut self) -> Result<()> {
        let hard_gas_limit = self
            .gas_per_alarm
            .checked_mul(self.alarms_per_message.into())
            .context("Failed to calculate hard gas limit for transaction")?;

        let mut fallback_gas = 0;

        loop {
            if self.alarms_status().await?.remaining_alarms {
                fallback_gas = self
                    .dispatch_alarms_streak(hard_gas_limit, fallback_gas)
                    .await?;
            }

            sleep(self.idle_duration).await;
        }
    }

    async fn alarms_status(&mut self) -> Result<AlarmsStatusResponse> {
        const QUERY_MSG: &[u8; 20] = br#"{"alarms_status":{}}"#;

        let address = self.contract.address().to_string();

        self.contract
            .query_wasm_mut()
            .smart(address, QUERY_MSG.to_vec())
            .await
    }

    async fn dispatch_alarms_streak(
        &mut self,
        hard_gas_limit: Gas,
        mut fallback_gas_per_alarm: Gas,
    ) -> Result<Gas> {
        loop {
            let Some(response) = self
                .broadcast(hard_gas_limit, fallback_gas_per_alarm)
                .await?
            else {
                log!(error![self]("Failed to fetch delivered transaction!"));

                continue;
            };

            let code: TxCode = response.code.into();

            let dispatched_alarms = if code.is_ok() {
                let dispatched_alarms: DispatchAlarmsResponse =
                    tx::decode_execute_response(&response)?;

                log_with_hash!(info![self, response](
                    "Dispatched {dispatched_alarms} alarms.",
                ));

                dispatched_alarms
            } else if code.value() == tx::OUT_OF_GAS_ERROR_CODE {
                log_with_hash!(warn![self, response](
                    log = ?response.raw_log,
                    "Transaction failed, likely because it ran out of gas.",
                ));

                self.alarms_per_message
            } else {
                log_with_hash!(error![self, response](
                    log = ?response.raw_log,
                    "Transaction failed because of unknown reason!",
                ));

                continue;
            };

            if let Some(gas_used_per_alarm) = response
                .gas_used
                .unsigned_abs()
                .checked_div(dispatched_alarms.into())
            {
                fallback_gas_per_alarm = tx::adjust_fallback_gas(
                    fallback_gas_per_alarm,
                    gas_used_per_alarm,
                )?;
            }

            if self.gas_per_alarm < fallback_gas_per_alarm {
                log!(warn![self](
                    %fallback_gas_per_alarm,
                    limit = %self.gas_per_alarm,
                    "Fallback gas exceeds gas limit per alarm! Clamping down!",
                ));

                fallback_gas_per_alarm = self.gas_per_alarm;
            }

            if dispatched_alarms < self.alarms_per_message {
                log!(info![self]("Entering idle mode."));

                break Ok(fallback_gas_per_alarm);
            }
        }
    }

    async fn broadcast(
        &mut self,
        hard_gas_limit: Gas,
        fallback_gas_per_alarm: Gas,
    ) -> Result<Option<TxResponse>> {
        let response_receiver =
            self.send_for_broadcasting(hard_gas_limit, fallback_gas_per_alarm)?;

        tx::fetch_delivered(
            &mut self.query_tx,
            &self.source,
            response_receiver.await?,
            self.timeout_duration,
        )
        .await
    }

    fn send_for_broadcasting(
        &mut self,
        hard_gas_limit: Gas,
        fallback_gas_per_alarm: Gas,
    ) -> Result<oneshot::Receiver<TxResponse>> {
        let (response_sender, response_receiver) = oneshot::channel();

        self.transaction_tx
            .send(TxPackage {
                tx_body: (*self.tx_body).clone(),
                source: self.source.clone(),
                hard_gas_limit,
                fallback_gas: fallback_gas_per_alarm
                    .wrapping_mul(self.alarms_per_message.into()),
                feedback_sender: response_sender,
                expiration: NoExpiration,
            })
            .map(|()| response_receiver)
            .context("Failed to send transaction for broadcasting!")
    }
}

impl<T> Runnable for AlarmsGenerator<T>
where
    T: Alarms,
{
    async fn run(self, _: RunnableState) -> Result<()> {
        self.dispatch_alarms().await
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[repr(transparent)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
pub(crate) struct AlarmsStatusResponse {
    pub remaining_alarms: bool,
}

#[derive(Clone)]
#[repr(transparent)]
pub struct PriceAlarms {
    protocol: Arc<str>,
}

impl PriceAlarms {
    #[inline]
    pub const fn new(protocol: Arc<str>) -> Self {
        Self { protocol }
    }

    #[inline]
    pub const fn protocol(&self) -> &Arc<str> {
        &self.protocol
    }
}

impl Alarms for PriceAlarms {
    const TYPE: &'static str = "Price";
    type Contract = GeneralizedOracle;
}

#[derive(Clone, Copy)]
pub struct TimeAlarms;

impl Alarms for TimeAlarms {
    const TYPE: &'static str = "Time";
    type Contract = contract::TimeAlarms;
}

type DispatchAlarmsResponse = u32;
