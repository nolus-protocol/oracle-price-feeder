use std::time::Duration;

use anyhow::Error as AnyError;
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

use alarms_dispatcher::error::{ContextError, WithOriginContext};
use alarms_dispatcher::{
    account::{account_data, account_id},
    client::Client,
    configuration::{read_config, Config, Node},
    context_message,
    error::{Error, WithCallerContext},
    messages::{
        ExecuteMsg, ExecuteResponse, OracleDispatchResponse, OracleStatusResponse, QueryMsg,
        QueryResponse,
    },
    signer::Signer,
    tx::ContractTx,
};

pub const DEFAULT_COSMOS_HD_PATH: &str = "m/44'/118'/0'/0/0";

pub const MAX_CONSEQUENT_ERRORS_COUNT: usize = 5;

type Result<T> = std::result::Result<T, ContextError<AnyError>>;

#[tokio::main]
async fn main() -> Result<()> {
    let log_writer = tracing_appender::rolling::hourly("./dispatcher-logs", "log");

    let (log_writer, _guard) = tracing_appender::non_blocking(log_writer);

    setup_logging(log_writer)?;

    let result = dispatch_alarms(prepare_rpc().await?)
        .await
        .with_caller_context(context_message!("Dispatcher loop exited with an error!"));

    if let Err(error) = &result {
        error!("{error}");
    }

    info!("Shutting down...");

    result
}

fn setup_logging<W>(writer: W) -> Result<()>
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
    .map_err(|error| {
        AnyError::from(error).with_origin_context(context_message!(
            "Couldn't register global default tracing dispatcher!"
        ))
    })
}

pub async fn signing_key(derivation_path: &str, password: &str) -> Result<SigningKey> {
    println!("Enter dispatcher's account secret: ");

    let mut secret = String::new();

    // Returns number of read bytes, which is meaningless for current case.
    let _ = BufReader::new(stdin())
        .read_line(&mut secret)
        .await
        .map_err(|error| {
            AnyError::from(error).with_origin_context(context_message!(
                "Couldn't read secret mnemonic from the standard input!"
            ))
        })?;

    SigningKey::derive_from_path(
        Mnemonic::new(secret.trim(), Language::English)
            .map_err(|error| {
                AnyError::from(error).with_origin_context(context_message!(
                    "Invalid mnemonic passed or is not in English!"
                ))
            })?
            .to_seed(password),
        &derivation_path.parse().map_err(|error| {
            AnyError::from(error)
                .with_origin_context(context_message!("Couldn't parse derivation path!"))
        })?,
    )
    .map_err(|error| {
        AnyError::from(error).with_origin_context(context_message!("Couldn't derive signing key!"))
    })
}

pub struct RpcSetup {
    signer: Signer,
    config: Config,
    client: Client,
}

async fn prepare_rpc() -> Result<RpcSetup> {
    let signing_key = signing_key(DEFAULT_COSMOS_HD_PATH, "")
        .await
        .with_caller_context(context_message!(
            "Something went wrong while preparing signing key!"
        ))?;

    info!("Successfully derived private key.");

    let config = read_config().await.with_caller_context(context_message!(
        "Something went wrong while load configuration!"
    ))?;

    info!("Successfully read configuration file.");

    let client = Client::new(config.node())
        .await
        .map_err(ContextError::map)
        .with_caller_context(context_message!(
            "Something went wrong while constructing client services!"
        ))?;

    info!("Fetching account data from network...");

    let account_id = account_id(&signing_key, config.node())
        .map_err(ContextError::map)
        .with_caller_context(context_message!(
            "Something went wrong while deriving account ID!"
        ))?;

    let account_data = account_data(account_id.clone(), &client)
        .await
        .map_err(ContextError::map)
        .with_caller_context(context_message!(
            "Something went wrong while fetching account data!"
        ))?;

    info!("Successfully fetched account data from network.");

    Ok(RpcSetup {
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
    RpcSetup {
        mut signer,
        config,
        client,
    }: RpcSetup,
) -> Result<()> {
    let poll_period = Duration::from_secs(config.poll_period_seconds());

    let query = serde_json_wasm::to_vec(&QueryMsg::Status {}).map_err(|error| {
        AnyError::from(error)
            .with_origin_context(context_message!("Serializing of query message failed!"))
    })?;

    loop {
        // TODO uncomment & refactor accordingly when after discussions
        //  about implementation
        // let time_alarms_response = dispatch_alarm::<TimeAlarmsResponse>(
        //     &mut signer,
        //     &client,
        //     config.node(),
        //     config.time_alarms().address(),
        //     config.time_alarms().max_alarms_group(),
        //     query_data.clone(),
        //     "time",
        // )
        // .await?;

        dispatch_alarm::<OracleStatusResponse, OracleDispatchResponse>(
            &mut signer,
            &client,
            config.node(),
            config.market_price_oracle().address(),
            config.market_price_oracle().max_alarms_group(),
            &query,
            "market price",
        )
        .await
        .with_caller_context("Something went wrong while dispatching market price alarm!")?;

        // TODO uncomment when after discussions about implementation
        // sleep_with_response(&time_alarms_response, poll_period).await;

        sleep(poll_period).await;
    }
}

async fn dispatch_alarm<'r, Q, E>(
    signer: &'r mut Signer,
    client: &'r Client,
    config: &'r Node,
    address: &'r str,
    max_alarms: u32,
    query: &'r [u8],
    alarm_type: &'static str,
) -> Result<()>
where
    Q: QueryResponse,
    E: ExecuteResponse,
{
    loop {
        let response: Q = serde_json_wasm::from_slice(
            &client
                .with_grpc({
                    let query_data = query.to_vec();

                    move |rpc| async move {
                        WasmQueryClient::new(rpc)
                            .smart_contract_state(QuerySmartContractStateRequest {
                                address: address.into(),
                                query_data,
                            })
                            .await
                            .map_err(|error| {
                                AnyError::from(error).with_origin_context(context_message!(
                                    "Status fetching query failed due to a connection error!"
                                ))
                            })
                    }
                })
                .await?
                .into_inner()
                .data,
        )
        .map_err(|error| {
            AnyError::from(error).with_origin_context(context_message!(
                "Deserialization of query response from JSON failed!"
            ))
        })?;

        if response.remaining_for_dispatch() {
            let result: E = commit_tx(signer, client, config, address, max_alarms)
                .await
                .with_caller_context(context_message!(
                    "Something went wrong while committing transaction!"
                ))?;

            info!(
                "Dispatched {} {} alarms.",
                result.dispatched_alarms(),
                alarm_type
            );

            if result.dispatched_alarms() == max_alarms {
                continue;
            }
        }

        return Ok(());
    }
}

async fn commit_tx<E>(
    signer: &mut Signer,
    client: &Client,
    config: &Node,
    address: &str,
    max_count: u32,
) -> Result<E>
where
    E: ExecuteResponse,
{
    let tx = ContractTx::new(address.into())
        .add_message(
            serde_json_wasm::to_vec(&ExecuteMsg::DispatchAlarms { max_count }).map_err(
                |error| {
                    AnyError::from(error).with_origin_context(context_message!(
                        "Serializing dispatch message to JSON failed!"
                    ))
                },
            )?,
            Vec::new(),
        )
        .commit(
            signer,
            Fee::from_amount_and_gas(config.fee().clone(), config.gas_limit_per_alarm()),
            None,
            None,
        )
        .map_err(ContextError::map)
        .with_caller_context(context_message!(
            "Something went wrong while committing message!"
        ))?;

    let tx_commit_response = client
        .with_json_rpc(|rpc| async move { tx.broadcast_commit(&rpc).await })
        .await
        .map_err(|error| {
            AnyError::from(Error::BroadcastTx(error)).with_origin_context(context_message!(
                "Error occurred while broadcasting commit!"
            ))
        })?;

    let response =
        serde_json_wasm::from_slice(&tx_commit_response.deliver_tx.data).map_err(|error| {
            AnyError::from(error).with_origin_context(context_message!(
                "Deserialization of dispatch message response from JSON failed!"
            ))
        })?;

    signer.tx_confirmed();

    Ok(response)
}

// TODO uncomment & refactor accordingly when after discussions
//  about implementation
// async fn sleep_with_response(response: &TimeAlarmsResponse, poll_period: Duration) {
//     sleep(
//         handle_time_alarms_response(response)
//             .await
//             .unwrap_or(poll_period)
//             .min(poll_period),
//     )
//     .await;
// }
//
// async fn handle_time_alarms_response(response: &TimeAlarmsResponse) -> Option<Duration> {
//     if let TimeAlarmsResponse::NextAlarm { timestamp } = response {
//         let Some(until) = SystemTime::UNIX_EPOCH
//             .checked_add(Duration::from_nanos(timestamp.as_nanos())) else {
//             error!(unix_timestamp = %timestamp.as_nanos(), "Couldn't calculate time of next alarm!");
//
//             return None;
//         };
//
//         return log_error!(
//             until.duration_since(SystemTime::now()),
//             "Error occurred while calculating duration between current time and time of next alarm!"
//         )
//         .ok();
//     }
//
//     None
// }
