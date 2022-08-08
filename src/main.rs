use cosmrs::rpc::endpoint::broadcast::tx_commit::Response;
use market_data_feeder::{
    configuration::Config,
    cosmos::{broadcast_tx, CosmosClient, ExecuteMsg, Wallet},
    errors::FeederError,
    provider::{get_supported_denom_pairs, Provider, ProviderType, ProvidersFactory},
};
use std::{io, process::exit, str::FromStr, time::Duration};
use tokio::time;

pub const DEFAULT_COSMOS_HD_PATH: &str = "m/44'/118'/0'/0/0";

#[tokio::main]
async fn main() -> Result<(), FeederError> {
    println!("Enter feeder's account secret: ");
    let mut secret = String::new();
    io::stdin().read_line(&mut secret)?;

    let cfg = match read_config() {
        Ok(c) => c,
        Err(err) => {
            eprintln!("Can not read config file: {}", err);
            exit(1)
        }
    };

    let wallet = Wallet::new(secret.trim(), DEFAULT_COSMOS_HD_PATH)?;
    let client = CosmosClient::new(cfg.oracle.clone())?;

    let supported_denom_pairs = get_supported_denom_pairs(&client).await?;

    let mut providers: Vec<Box<dyn Provider>> = vec![];
    for provider_cfg in &cfg.providers {
        let p_type = match ProviderType::from_str(&provider_cfg.main_type) {
            Ok(t) => t,
            Err(_) => panic!("Unknown provider type {}", &provider_cfg.main_type),
        };

        let provider = match ProvidersFactory::new_provider(&p_type, provider_cfg) {
            Ok(p) => p,
            Err(err) => panic!("Can not create provider instance {:?}", err),
        };

        providers.push(provider);
    }

    let mut interval = time::interval(Duration::from_secs(cfg.tick_time));
    for _i in 0.. {
        interval.tick().await;
        for provider in &providers {
            let prices = match provider.get_spot_prices(&supported_denom_pairs).await {
                Ok(prices) => prices,
                Err(err) => return Err(err).map_err(FeederError::Provider),
            };
            if !prices.is_empty() {
                let price_feed_json = serde_json::to_string(&ExecuteMsg::FeedPrices { prices })?;
                print_tx_response(
                    broadcast_tx(&client, &wallet, &cfg.oracle, price_feed_json).await?,
                );
            }
        }
    }

    Ok(())
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

fn read_config() -> std::io::Result<Config> {
    let content = std::fs::read_to_string("market-data-feeder.toml")?;
    Ok(toml::from_str(&content)?)
}
