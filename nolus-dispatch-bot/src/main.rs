use std::{
    io::stdin,
    process::exit,
    thread::sleep,
    time::{Duration, SystemTime},
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

#[tokio::main]
async fn main() {
    setup_logging();

    let wallet = get_wallet();

    info!("Successfully derived private key.");

    let cfg = read_config();

    info!("Successfully read configuration file.");

    let client = Client::new(cfg.oracle.clone()).unwrap_or_else(|error| {
        error!(
            error = %error,
            "Error occurred while connecting to node! Invalid URL provided!"
        );

        exit(1);
    });
    let oracle = cfg.oracle;

    info!("Fetching account data from network...");

    let sender_account_id = get_sender_account_id(&wallet, &oracle).unwrap_or_else(|error| {
        error!(
            error = %error,
            "Error occurred while fetching sender account's ID!"
        );

        exit(1);
    });
    let account_data = get_account_data(&client, &sender_account_id)
        .await
        .unwrap_or_else(|error| {
            error!(
                error = %error,
                "Error occurred while fetching account data!"
            );

            exit(1);
        });

    let rpc_client = construct_rpc_client(&oracle).unwrap_or_else(|error| {
        error!(
            error = %error,
            "Error occurred while constructing RPC client!"
        );

        exit(1);
    });

    let json_data: String = serde_json_wasm::to_string(&ExecuteMsg::DispatchAlarms {
        max_count: cfg.max_alarms_in_transaction,
    })
    .unwrap_or_else(|error| {
        error!(
            error = %error,
            "Couldn't serialize alarm dispatch message as JSON!"
        );

        exit(1);
    });

    let mut consequent_error_count: usize = 0;

    loop {
        if consequent_error_count == 1 {
            error!(
                "{} consequent errors encountered! Shutting down...",
                consequent_error_count
            );

            exit(1);
        }

        let Some(response) = commit_tx(&sender_account_id, &account_data, &wallet, &oracle, &json_data, &rpc_client).await else {
            consequent_error_count += 1;

            continue;
        };

        consequent_error_count = 0;

        match response {
            AlarmsResponse::RemainingForDispatch {} => {}
            AlarmsResponse::NextAlarm { unix_time } => {
                let Some(until) = SystemTime::UNIX_EPOCH
                    .checked_add(Duration::from_nanos(unix_time)) else {
                    error!(unix_timestamp = %unix_time, "Couldn't calculate time of next alarm...");

                    continue;
                };

                if let Err(error) = until.duration_since(SystemTime::now()).map(sleep) {
                    error!(
                        error = %error,
                        "Error occurred while calculating duration between current time and time of next alarm..."
                    );

                    continue;
                };
            }
        }
    }
}

fn setup_logging() {
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
    .unwrap_or_else(|error| {
        error!(
            error = %error,
            "Couldn't register global default tracing dispatcher!"
        );

        exit(1);
    })
}

fn get_wallet() -> Wallet {
    println!("Enter feeder's account secret: ");
    let mut secret = String::new();
    stdin().read_line(&mut secret).unwrap_or_else(|error| {
        error!(
            error = %error,
            "Couldn't read secret mnemonic from the standard input!"
        );

        exit(1);
    });

    Wallet::new(secret.trim(), DEFAULT_COSMOS_HD_PATH).unwrap_or_else(|error| {
        error!(
            error = %error,
            "Couldn't derive secret key from mnemonic!"
        );

        exit(1);
    })
}

async fn commit_tx(
    sender_account_id: &AccountId,
    account_data: &BaseAccount,
    wallet: &Wallet,
    config: &Oracle,
    data: &str,
    rpc_client: &HttpClient,
) -> Option<AlarmsResponse> {
    let tx = match construct_tx(
        sender_account_id,
        account_data,
        wallet,
        config,
        String::from(data),
    ) {
        Ok(tx) => tx,
        Err(error) => {
            error!(
                error = %error,
                "Error occurred while constructing transaction!"
            );

            return None;
        }
    };

    let tx_commit_response = match tx.broadcast_commit(rpc_client).await {
        Ok(tx_commit_response) => tx_commit_response,
        Err(error) => {
            error!(
                error = %error,
                "Error occurred while broadcasting commit!"
            );

            return None;
        }
    };

    let Some(response) = (match tx_commit_response
        .deliver_tx
        .data
        .map(Into::<Vec<u8>>::into)
        .as_deref()
        .map(serde_json_wasm::from_slice::<AlarmsResponse>)
        .transpose() {
        Ok(response) => response,
        Err(error) => {
            error!(
                    error = %error,
                    "Error occurred while parsing returned data!"
                );

            return None;
        }
    }) else {
        error!("Contract did not return any data!");

        return None;
    };

    Some(response)
}
