use std::{io, process::exit, str::FromStr, time::Duration};

use cosmrs::rpc::endpoint::broadcast::tx_commit::Response;
use tokio::time;

use market_data_feeder::{
    configuration::Config,
    cosmos::{broadcast_tx, CosmosClient, ExecuteMsg, Wallet},
    errors::FeederError,
    provider::{get_supported_denom_pairs, Provider, ProviderType, ProvidersFactory},
};

pub const DEFAULT_COSMOS_HD_PATH: &str = "m/44'/118'/0'/0/0";

#[tokio::main]
async fn main() -> Result<(), FeederError> {
    println!("Enter feeder's account secret: ");
    let mut secret = String::new();
    io::stdin().read_line(&mut secret)?;

    let cfg = read_config().unwrap_or_else(|err| {
        eprintln!("Can not read config file: {}", err);

        exit(1)
    });

    let wallet = Wallet::new(secret.trim(), DEFAULT_COSMOS_HD_PATH)?;
    let client = CosmosClient::new(cfg.oracle.clone())?;

    let supported_denom_pairs = get_supported_denom_pairs(&client).await?;

    let mut providers: Vec<Box<dyn Provider>> = vec![];
    for provider_cfg in &cfg.providers {
        let p_type = ProviderType::from_str(&provider_cfg.main_type)
            .unwrap_or_else(|()| panic!("Unknown provider type {}", &provider_cfg.main_type));

        let provider = ProvidersFactory::new_provider(&p_type, provider_cfg)
            .unwrap_or_else(|err| panic!("Can not create provider instance {:?}", err));

        providers.push(provider);
    }

    let mut interval = time::interval(Duration::from_secs(cfg.tick_time));

    loop {
        interval.tick().await;

        for provider in &providers {
            let prices = provider
                .get_spot_prices(&supported_denom_pairs)
                .await
                .map_err(FeederError::Provider)?;

            if !prices.is_empty() {
                let price_feed_json = serde_json::to_string(&ExecuteMsg::FeedPrices { prices })?;

                print_tx_response(
                    broadcast_tx(&client, &wallet, &cfg.oracle, price_feed_json).await?,
                );
            }
        }
    }
}

fn print_tx_response(tx_commit_response: Response) {
    println!("{}", tx_commit_response.hash);
    println!(
        "deliver_tx.gas_used {}",
        tx_commit_response.deliver_tx.gas_used
    );
    println!("check_tx.gas_used {}", tx_commit_response.check_tx.gas_used);
    println!("deliver_tx.log {}", tx_commit_response.deliver_tx.log);
    println!("check_tx.log {}", tx_commit_response.check_tx.log);
}

fn read_config() -> io::Result<Config> {
    std::fs::read_to_string("market-data-feeder.toml")
        .and_then(|content| toml::from_str(&content).map_err(Into::into))
}
