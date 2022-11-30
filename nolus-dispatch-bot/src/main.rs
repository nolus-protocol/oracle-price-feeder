use std::{
    num::NonZeroU32,
    time::{Duration, SystemTime},
};

use anyhow::{anyhow, bail, Context, Result as AnyResult};
use cosmrs::{
    bip32::{Language, Mnemonic},
    crypto::secp256k1::SigningKey,
    proto::cosmwasm::wasm::v1::{query_client::QueryClient, QuerySmartContractStateRequest},
    tx::Fee,
};
use tokio::{
    io::{stdin, AsyncBufReadExt, BufReader},
    time::sleep,
};
use tracing::{error, info, Dispatch};

use nolus_dispatch_bot::{
    account::{get_account_data, get_account_id},
    client::Client,
    configuration::{read_config, Config, Node},
    log_error,
    messages::{ExecuteMsg, OracleResponse, QueryMsg, Response, TimeAlarmsResponse},
    signing::Signer,
    tx::ContractMsgs,
};

pub const DEFAULT_COSMOS_HD_PATH: &str = "m/44'/118'/0'/0/0";

pub const MAX_CONSEQUENT_ERRORS_COUNT: usize = 5;

#[tokio::main]
async fn main() -> AnyResult<()> {
    setup_logging()?;

    log_error!(
        dispatch_alarms(log_error!(
            prepare_rpc_data().await,
            "Failed to prepare RPC data!"
        )?)
        .await,
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

pub async fn get_signing_key(derivation_path: &str, password: &str) -> AnyResult<SigningKey> {
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
    let signing_key = get_signing_key(DEFAULT_COSMOS_HD_PATH, "").await?;

    info!("Successfully derived private key.");

    let config = read_config().await?;

    info!("Successfully read configuration file.");

    let client = log_error!(
        Client::new(config.node()).await,
        "Error occurred while connecting to node! Invalid URL provided!"
    )?;

    info!("Fetching account data from network...");

    let account_id = log_error!(
        get_account_id(&signing_key, config.node()),
        "Couldn't derive account ID!"
    )?;

    let account_data = log_error!(
        get_account_data(account_id.clone(), &client).await,
        "Error occurred while fetching account data!"
    )?;

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
    let mut consequent_errors_count: usize = 0;

    let poll_period = Duration::from_secs(config.poll_period_seconds());

    loop {
        let time_alarms_response = log_error!(
            dispatch_alarm::<TimeAlarmsResponse>(
                &mut signer,
                &client,
                config.node(),
                config.time_alarms().address(),
                config.time_alarms().max_alarms_group()
            )
            .await,
            "Dispatching time alarms failed!"
        );

        let oracle_error = log_error!(
            dispatch_alarm::<OracleResponse>(
                &mut signer,
                &client,
                config.node(),
                config.market_price_oracle().address(),
                config.market_price_oracle().max_alarms_group()
            )
            .await,
            "Dispatching market price oracle alarms failed!"
        )
        .is_err();

        let sleep_duration = if let Ok(response) = &time_alarms_response {
            if let Ok(Some(duration)) = log_error!(
                handle_time_alarms_response(response).await,
                "Failed handling time alarms' response!"
            ) {
                duration.min(poll_period)
            } else {
                poll_period
            }
        } else {
            poll_period
        };

        consequent_errors_count = if time_alarms_response.is_err() || oracle_error {
            consequent_errors_count + 1
        } else {
            0
        };

        if consequent_errors_count > MAX_CONSEQUENT_ERRORS_COUNT {
            error!(
                "{} consequent errors encountered! Exiting dispatcher loop...",
                consequent_errors_count
            );

            bail!("Encountered {} consequent errors!", consequent_errors_count);
        }

        sleep(sleep_duration).await;
    }
}

async fn dispatch_alarm<T>(
    signer: &mut Signer,
    client: &Client,
    config: &Node,
    address: &str,
    max_alarms: u32,
) -> AnyResult<T>
where
    T: Response,
{
    let response: T = serde_json_wasm::from_slice::<T>(
        &client
            .with_grpc({
                let query_data = serde_json_wasm::to_vec(&QueryMsg::DispatchToAlarms {})?;

                move |rpc| async move {
                    QueryClient::new(rpc)
                        .smart_contract_state(QuerySmartContractStateRequest {
                            address: address.into(),
                            query_data,
                        })
                        .await
                }
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
    let tx = log_error!(
        ContractMsgs::new(address.into())
            .add_message(
                serde_json_wasm::to_vec(&ExecuteMsg::DispatchAlarms {
                    max_amount: alarms_to_dispatch.get()
                })?,
                Vec::new()
            )
            .commit(
                signer,
                Fee::from_amount_and_gas(config.fee().clone(), config.gas_limit_per_alarm()),
                None,
                None,
            ),
        "Error occurred while constructing transaction!"
    )?;

    let Ok(tx_commit_response) = log_error!(
        client.with_json_rpc(|rpc| async move {
            tx.broadcast_commit(&rpc).await
        }).await,
        "Error occurred while broadcasting commit!"
    ) else {
        bail!("Error while broadcasting");
    };

    let response = log_error!(
        log_error!(
            tx_commit_response
                .deliver_tx
                .data
                .map(Into::<Vec<u8>>::into)
                .as_deref()
                .map(serde_json_wasm::from_slice::<T>)
                .ok_or_else(|| anyhow!("No data returned!")),
            "Contract did not return any data!"
        )?,
        "Error occurred while parsing returned data!"
    )?;

    signer.tx_confirmed();

    Ok(response)
}

async fn handle_time_alarms_response(response: &TimeAlarmsResponse) -> AnyResult<Option<Duration>> {
    Ok(match response {
        TimeAlarmsResponse::RemainingForDispatch { .. } => None,
        &TimeAlarmsResponse::NextAlarm { unix_time } => {
            let Some(until) = SystemTime::UNIX_EPOCH
                .checked_add(Duration::from_nanos(unix_time)) else {
                error!(unix_timestamp = %unix_time, "Couldn't calculate time of next alarm...");

                bail!("Returned timestamp is outside valid range!");
            };

            Some(log_error!(
                until.duration_since(SystemTime::now()),
                "Error occurred while calculating duration between current time and time of next alarm..."
            )?)
        }
    })
}
