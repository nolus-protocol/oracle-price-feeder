use std::{
    num::NonZeroU32,
    time::{Duration, SystemTime},
};

use anyhow::{Context, Result as AnyResult};
use cosmrs::{
    bip32::{Language, Mnemonic},
    crypto::secp256k1::SigningKey,
    proto::cosmwasm::wasm::v1::{
        query_client::QueryClient as WasmQueryClient, QuerySmartContractStateRequest,
    },
    tx::Fee,
};
use tokio::{
    io::{stdin, AsyncBufReadExt, BufReader},
    time::sleep,
};
use tracing::{error, info, Dispatch};

use alarms_dispatcher::{
    account::{account_data, account_id},
    client::Client,
    configuration::{read_config, Config, Node},
    error::Error,
    log_error,
    messages::{ExecuteMsg, OracleResponse, QueryMsg, Response, TimeAlarmsResponse},
    signer::Signer,
    tx::ContractMsgs,
};

pub const DEFAULT_COSMOS_HD_PATH: &str = "m/44'/118'/0'/0/0";

pub const MAX_CONSEQUENT_ERRORS_COUNT: usize = 5;

#[tokio::main]
async fn main() -> AnyResult<()> {
    let appender = tracing_appender::rolling::hourly("./dispatcher-logs", "log");

    let (writer, _guard) = tracing_appender::non_blocking(appender);

    setup_logging(writer)?;

    log_error!(
        dispatch_alarms(prepare_rpc_data().await?).await,
        "Dispatcher loop exited with an error! Shutting down..."
    )
}

fn setup_logging<W>(writer: W) -> AnyResult<()>
where
    W: for<'r> tracing_subscriber::fmt::MakeWriter<'r> + Send + Sync + 'static,
{
    tracing::dispatcher::set_global_default(Dispatch::new(
        tracing_subscriber::fmt()
            .with_level(true)
            .with_ansi(true)
            .with_file(false)
            .with_line_number(false)
            .with_writer(writer)
            .with_max_level({
                #[cfg(debug_assertions)]
                {
                    tracing::level_filters::LevelFilter::DEBUG
                }
                #[cfg(not(debug_assertions))]
                {
                    use std::{env::var_os, ffi::OsStr};

                    if var_os("MARKET_DATA_FEEDER_DEBUG")
                        .map(|value| {
                            [OsStr::new("1"), OsStr::new("y"), OsStr::new("Y")]
                                .contains(&value.as_os_str())
                        })
                        .unwrap_or_default()
                    {
                        tracing::level_filters::LevelFilter::DEBUG
                    } else {
                        tracing::level_filters::LevelFilter::INFO
                    }
                }
            })
            .finish(),
    ))
    .with_context(|| "Couldn't register global default tracing dispatcher!".to_string())
}

pub async fn signing_key(derivation_path: &str, password: &str) -> AnyResult<SigningKey> {
    println!("Enter dispatcher's account secret: ");

    let mut secret = String::new();

    // Returns number of read bytes, which is meaningless for current case.
    let _ = log_error!(
        BufReader::new(stdin()).read_line(&mut secret).await,
        "Couldn't read secret mnemonic from the standard input!"
    )?;

    SigningKey::derive_from_path(
        Mnemonic::new(secret.trim(), Language::English)?.to_seed(password),
        &derivation_path.parse()?,
    )
    .map_err(Into::into)
}

pub struct RpcData {
    signer: Signer,
    config: Config,
    client: Client,
}

async fn prepare_rpc_data() -> AnyResult<RpcData> {
    let signing_key = signing_key(DEFAULT_COSMOS_HD_PATH, "").await?;

    info!("Successfully derived private key.");

    let config = read_config().await?;

    info!("Successfully read configuration file.");

    let client = Client::new(config.node()).await?;

    info!("Fetching account data from network...");

    let account_id = account_id(&signing_key, config.node())?;

    let account_data = account_data(account_id.clone(), &client).await?;

    info!("Successfully fetched account data from network.");

    Ok(RpcData {
        signer: Signer::new(
            account_id.to_string(),
            signing_key,
            config.node().chain_id().clone(),
            account_data,
        ),
        config,
        client,
    })
}

async fn dispatch_alarms(
    RpcData {
        mut signer,
        config,
        client,
    }: RpcData,
) -> AnyResult<()> {
    let poll_period = Duration::from_secs(config.poll_period_seconds());

    let query_data = serde_json_wasm::to_vec(&QueryMsg::Status {})?;

    loop {
        let time_alarms_response = dispatch_alarm::<TimeAlarmsResponse>(
            &mut signer,
            &client,
            config.node(),
            config.time_alarms().address(),
            config.time_alarms().max_alarms_group(),
            query_data.clone(),
        )
        .await?;

        dispatch_alarm::<OracleResponse>(
            &mut signer,
            &client,
            config.node(),
            config.market_price_oracle().address(),
            config.market_price_oracle().max_alarms_group(),
            query_data.clone(),
        )
        .await?;

        sleep_with_response(&time_alarms_response, poll_period).await;
    }
}

async fn dispatch_alarm<T>(
    signer: &mut Signer,
    client: &Client,
    config: &Node,
    address: &str,
    max_alarms: u32,
    query_data: Vec<u8>,
) -> AnyResult<T>
where
    T: Response,
{
    let response: T = serde_json_wasm::from_slice(
        &client
            .with_grpc(move |rpc| async move {
                WasmQueryClient::new(rpc)
                    .smart_contract_state(QuerySmartContractStateRequest {
                        address: address.into(),
                        query_data,
                    })
                    .await
            })
            .await?
            .into_inner()
            .data,
    )?;

    if let Some(alarms_to_dispatch) = NonZeroU32::new(
        response
            .remaining_for_dispatch()
            .unwrap_or_default()
            .min(max_alarms),
    ) {
        commit_tx(signer, client, config, address, alarms_to_dispatch).await
    } else {
        Ok(response)
    }
}

async fn commit_tx<T>(
    signer: &mut Signer,
    client: &Client,
    config: &Node,
    address: &str,
    alarms_to_dispatch: NonZeroU32,
) -> AnyResult<T>
where
    T: Response,
{
    let tx = ContractMsgs::new(address.into())
        .add_message(
            serde_json_wasm::to_vec(&ExecuteMsg::DispatchAlarms {
                max_count: alarms_to_dispatch.get(),
            })?,
            Vec::new(),
        )
        .commit(
            signer,
            Fee::from_amount_and_gas(config.fee().clone(), config.gas_limit_per_alarm()),
            None,
            None,
        )?;

    let tx_commit_response = log_error!(
        client
            .with_json_rpc(|rpc| async move { tx.broadcast_commit(&rpc).await })
            .await,
        "Error occurred while broadcasting commit!"
    )
    .map_err(Error::BroadcastTx)?;

    let response = log_error!(
        serde_json_wasm::from_slice(&tx_commit_response.deliver_tx.data),
        "Error occurred while parsing returned data!"
    )?;

    signer.tx_confirmed();

    Ok(response)
}

async fn sleep_with_response(response: &TimeAlarmsResponse, poll_period: Duration) {
    sleep(
        handle_time_alarms_response(response)
            .await
            .unwrap_or(poll_period)
            .min(poll_period),
    )
    .await;
}

async fn handle_time_alarms_response(response: &TimeAlarmsResponse) -> Option<Duration> {
    if let TimeAlarmsResponse::NextAlarm { timestamp } = response {
        let Some(until) = SystemTime::UNIX_EPOCH
            .checked_add(Duration::from_nanos(timestamp.as_nanos())) else {
            error!(unix_timestamp = %timestamp.as_nanos(), "Couldn't calculate time of next alarm!");

            return None;
        };

        return log_error!(
            until.duration_since(SystemTime::now()),
            "Error occurred while calculating duration between current time and time of next alarm!"
        )
        .ok();
    }

    None
}
