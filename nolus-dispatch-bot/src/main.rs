use std::time::{Duration, SystemTime};

use anyhow::{anyhow, bail, Context, Result as AnyResult};
use tokio::{
    io::{stdin, AsyncBufReadExt, BufReader},
    time::sleep,
};
use tracing::{error, info, Dispatch};

use market_data_feeder::{
    configuration::Oracle,
    cosmos::{
        construct_rpc_client, construct_tx, get_account_data, get_sender_account_id,
        AlarmsResponse, Client, ExecuteMsg, Wallet,
    },
    cosmos_sdk_proto::cosmos::auth::v1beta1::BaseAccount,
    cosmrs::{rpc::HttpClient, AccountId},
};

use crate::configuration::read_config;

mod configuration;

pub const DEFAULT_COSMOS_HD_PATH: &str = "m/44'/118'/0'/0/0";

pub const MAX_CONSEQUENT_ERRORS_COUNT: usize = 5;

#[macro_export]
macro_rules! log_error {
    ($expr: expr, $error: literal $(, $args: expr)* $(,)?) => {{
        let result: Result<_, _> = $expr;

        if let Err(error) = &result {
            ::tracing::error!(
                error = ?error,
                $error
                $(, $args)*,
            );
        }

        result
    }};
}

#[tokio::main]
async fn main() -> AnyResult<()> {
    setup_logging()?;

    let rpc_data = log_error!(prepare_rpc_data().await, "Failed to prepare RPC data!")?;

    let rpc_client = log_error!(
        construct_rpc_client(&rpc_data.oracle),
        "Error occurred while constructing RPC client!"
    )?;

    log_error!(
        dispatch_alarms(rpc_data, rpc_client).await,
        "Dispatcher loop exited with an error! Shutting down..."
    )
}

fn setup_logging() -> AnyResult<()> {
    tracing::dispatcher::set_global_default(Dispatch::new(
        tracing_subscriber::fmt()
            .with_level(true)
            .with_ansi(true)
            .with_file(true)
            .with_line_number(true)
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
    .with_context(|| format!("Couldn't register global default tracing dispatcher!"))
}

async fn get_wallet() -> AnyResult<Wallet> {
    println!("Enter dispatcher's account secret: ");

    let mut secret = String::new();

    // Returns number of read bytes, which is meaningless for current case.
    let _ = log_error!(
        BufReader::new(stdin()).read_line(&mut secret).await,
        "Couldn't read secret mnemonic from the standard input!"
    )?;

    Wallet::new(secret.trim(), DEFAULT_COSMOS_HD_PATH).map_err(Into::into)
}

pub struct RpcData {
    wallet: Wallet,
    oracle: Oracle,
    account_id: AccountId,
    account_data: BaseAccount,
    tx_message: String,
}

async fn prepare_rpc_data() -> AnyResult<RpcData> {
    let wallet = get_wallet().await?;

    info!("Successfully derived private key.");

    let cfg = read_config().await?;

    info!("Successfully read configuration file.");

    let client = log_error!(
        Client::new(cfg.oracle.clone()),
        "Error occurred while connecting to node! Invalid URL provided!"
    )?;
    let oracle = cfg.oracle;

    info!("Fetching account data from network...");

    let account_id = log_error!(
        get_sender_account_id(&wallet, &oracle),
        "Error occurred while fetching sender account's ID!"
    )?;

    let account_data = log_error!(
        get_account_data(&client, &account_id).await,
        "Error occurred while fetching account data!"
    )?;

    let tx_message: String = log_error!(
        serde_json_wasm::to_string(&ExecuteMsg::DispatchAlarms {
            max_count: cfg.max_alarms_in_transaction,
        }),
        "Couldn't serialize alarm dispatch message as JSON!"
    )?;

    Ok(RpcData {
        wallet,
        oracle,
        account_id,
        account_data,
        tx_message,
    })
}

async fn dispatch_alarms(
    RpcData {
        wallet,
        oracle,
        account_id,
        account_data,
        tx_message,
    }: RpcData,
    rpc_client: HttpClient,
) -> AnyResult<()> {
    let mut consequent_errors_count: usize = 0;

    loop {
        if consequent_errors_count == MAX_CONSEQUENT_ERRORS_COUNT {
            error!(
                "{} consequent errors encountered! Exiting dispatcher loop...",
                consequent_errors_count
            );

            bail!("Encountered {} consequent errors!", consequent_errors_count);
        }

        let Ok(response) = log_error!(
            commit_tx(
                &account_id,
                &account_data,
                &wallet,
                &oracle,
                &tx_message,
                &rpc_client,
            ).await,
            "Failed to commit transaction!"
        ) else {
            consequent_errors_count += 1;

            continue;
        };

        if log_error!(handle_response(response).await, "Failed handling response!").is_err() {
            consequent_errors_count += 1;

            continue;
        }

        consequent_errors_count = 0;
    }
}

async fn commit_tx(
    sender_account_id: &AccountId,
    account_data: &BaseAccount,
    wallet: &Wallet,
    config: &Oracle,
    data: &str,
    rpc_client: &HttpClient,
) -> AnyResult<AlarmsResponse> {
    let tx = log_error!(
        construct_tx(
            sender_account_id,
            account_data,
            wallet,
            config,
            String::from(data),
        ),
        "Error occurred while constructing transaction!"
    )?;

    let Ok(tx_commit_response) = log_error!(
        tx.broadcast_commit(rpc_client).await,
        "Error occurred while broadcasting commit!"
    ) else {
        bail!("Error occurred while broadcasting commit!");
    };

    log_error!(
        log_error!(
            tx_commit_response
                .deliver_tx
                .data
                .map(Into::<Vec<u8>>::into)
                .as_deref()
                .map(serde_json_wasm::from_slice::<AlarmsResponse>)
                .ok_or_else(|| anyhow!("No data returned!")),
            "Contract did not return any data!"
        )?,
        "Error occurred while parsing returned data!"
    )
    .map_err(Into::into)
}

async fn handle_response(response: AlarmsResponse) -> AnyResult<()> {
    match response {
        AlarmsResponse::RemainingForDispatch {} => {}
        AlarmsResponse::NextAlarm { unix_time } => {
            let Some(until) = SystemTime::UNIX_EPOCH
                .checked_add(Duration::from_nanos(unix_time)) else {
                error!(unix_timestamp = %unix_time, "Couldn't calculate time of next alarm...");

                bail!("Returned timestamp is outside valid range!");
            };

            log_error!(
                until.duration_since(SystemTime::now()).map(sleep),
                "Error occurred while calculating duration between current time and time of next alarm..."
            )?.await;
        }
    }

    Ok(())
}
