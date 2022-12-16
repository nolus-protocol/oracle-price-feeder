use std::time::Duration;

use cosmrs::{
    bip32::{Language, Mnemonic},
    crypto::secp256k1::SigningKey,
    proto::cosmwasm::wasm::v1::{
        query_client::QueryClient as WasmQueryClient, QuerySmartContractStateRequest,
    },
    tx::Fee,
};
use tokio::{
    io::{AsyncBufReadExt, BufReader as AsyncBufReader},
    time::sleep,
};
use tracing::{debug, error, info, Dispatch};

use alarms_dispatcher::{
    account::{account_data, account_id},
    client::Client,
    configuration::{read_config, Config, Node},
    messages::{DispatchResponse, ExecuteMsg, QueryMsg, StatusResponse},
    signer::Signer,
    tx::ContractTx,
};

pub mod error;
pub mod log;

pub const DEFAULT_COSMOS_HD_PATH: &str = "m/44'/118'/0'/0/0";

pub const MAX_CONSEQUENT_ERRORS_COUNT: usize = 5;

#[tokio::main]
async fn main() -> Result<(), error::Application> {
    let log_writer = tracing_appender::rolling::hourly("./dispatcher-logs", "log");

    let (log_writer, _guard) =
        tracing_appender::non_blocking(log::CombinedWriter::new(std::io::stdout(), log_writer));

    setup_logging(log_writer)?;

    info!(concat!(
        "Running version built on: ",
        env!("BUILD_START_TIME_DATE", "No build time provided!")
    ));

    let result = dispatch_alarms(prepare_rpc().await?).await;

    if let Err(error) = &result {
        error!("{error}");
    }

    info!("Shutting down...");

    result.map_err(Into::into)
}

fn setup_logging<W>(writer: W) -> Result<(), tracing::dispatcher::SetGlobalDefaultError>
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

                    if var_os("ALARMS_DISPATCHER_DEBUG")
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
}

pub async fn signing_key(
    derivation_path: &str,
    password: &str,
) -> Result<SigningKey, error::SigningKey> {
    use error::SigningKey as Error;

    println!("Enter dispatcher's account secret: ");

    let mut secret = String::new();

    // Returns number of read bytes, which is meaningless for current case.
    let _ = AsyncBufReader::new(tokio::io::stdin())
        .read_line(&mut secret)
        .await?;

    SigningKey::derive_from_path(
        Mnemonic::new(secret.trim(), Language::English)
            .map_err(Error::ParsingMnemonic)?
            .to_seed(password),
        &derivation_path
            .parse()
            .map_err(Error::ParsingDerivationPath)?,
    )
    .map_err(Error::DerivingKey)
}

pub struct RpcSetup {
    signer: Signer,
    config: Config,
    client: Client,
}

async fn prepare_rpc() -> Result<RpcSetup, error::RpcSetup> {
    let signing_key = signing_key(DEFAULT_COSMOS_HD_PATH, "").await?;

    info!("Successfully derived private key.");

    let config = read_config().await?;

    info!("Successfully read configuration file.");

    let client = Client::new(config.node()).await?;

    info!("Fetching account data from network...");

    let account_id = account_id(&signing_key, config.node())?;

    let account_data = account_data(account_id.clone(), &client).await?;

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
) -> Result<(), error::DispatchAlarms> {
    let poll_period = Duration::from_secs(config.poll_period_seconds());

    let query = serde_json_wasm::to_vec(&QueryMsg::AlarmsStatus {})?;

    loop {
        for (contract, type_name) in [
            (config.market_price_oracle(), "market price"),
            (config.time_alarms(), "time"),
        ] {
            dispatch_alarm(
                &mut signer,
                &client,
                config.node(),
                contract.address(),
                contract.max_alarms_group(),
                &query,
                type_name,
            )
            .await?;
        }

        sleep(poll_period).await;
    }
}

async fn dispatch_alarm<'r>(
    signer: &'r mut Signer,
    client: &'r Client,
    config: &'r Node,
    address: &'r str,
    max_alarms: u32,
    query: &'r [u8],
    alarm_type: &'static str,
) -> Result<(), error::DispatchAlarm> {
    loop {
        let response: StatusResponse = query_status(client, address, query).await?;

        if response.remaining_for_dispatch() {
            let result = commit_tx(signer, client, config, address, max_alarms).await?;

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

async fn query_status(
    client: &Client,
    address: &str,
    query: &[u8],
) -> Result<StatusResponse, error::StatusQuery> {
    serde_json_wasm::from_slice(&{
        let data = client
            .with_grpc({
                let query_data = query.to_vec();

                move |rpc| async move {
                    WasmQueryClient::new(rpc)
                        .smart_contract_state(QuerySmartContractStateRequest {
                            address: address.into(),
                            query_data,
                        })
                        .await
                }
            })
            .await?
            .into_inner()
            .data;

        debug!(
            data = %String::from_utf8_lossy(&data),
            "gRPC status response from {address} returned successfully!",
        );

        data
    })
    .map_err(Into::into)
}

async fn commit_tx(
    signer: &mut Signer,
    client: &Client,
    config: &Node,
    address: &str,
    max_count: u32,
) -> Result<DispatchResponse, error::TxCommit> {
    let tx = ContractTx::new(address.into())
        .add_message(
            serde_json_wasm::to_vec(&ExecuteMsg::DispatchAlarms { max_count })?,
            Vec::new(),
        )
        .commit(
            signer,
            Fee::from_amount_and_gas(config.fee().clone(), config.gas_limit_per_alarm()),
            None,
            None,
        )?;

    let tx_commit_response = client
        .with_json_rpc(|rpc| async move { tx.broadcast_commit(&rpc).await })
        .await?;

    let response = serde_json_wasm::from_slice(&tx_commit_response.deliver_tx.data)?;

    signer.tx_confirmed();

    Ok(response)
}
