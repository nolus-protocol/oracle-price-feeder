use market_data_feeder::{
    configuration::Config,
    cosm_client::CosmClient,
    errors::FeederError,
    provider::{get_supported_denom_pairs, ProviderType, ProvidersFactory},
};
use std::{io, process::exit, str::FromStr};

fn main() -> Result<(), FeederError> {
    println!("Enter feeder's account secret: ");
    let mut secret = String::new();
    io::stdin().read_line(&mut secret).unwrap();

    let cfg = match read_config() {
        Ok(c) => c,
        Err(err) => {
            eprintln!("Can not read config file: {}", err);
            exit(1)
        }
    };

    let cosm_client = match CosmClient::new(&cfg.oracle, secret.trim().to_string()) {
        Ok(c) => c,
        Err(err) => {
            panic!("Can not create cosmos client: {}", err);
        }
    };

    let rt = tokio::runtime::Runtime::new().unwrap();
    for provider_cfg in &cfg.providers {
        let p_type = match ProviderType::from_str(&provider_cfg.main_type) {
            Ok(t) => t,
            Err(_) => panic!("Unknown provider type {}", &provider_cfg.main_type),
        };

        let provider = match ProvidersFactory::new_provider(&p_type, provider_cfg) {
            Ok(p) => p,
            Err(err) => panic!("Can not create provider instance {:?}", err),
        };

        let denoms_future = get_supported_denom_pairs(&cosm_client);
        let output = rt.block_on(denoms_future);
        let supported_denom_pairs = match output {
            Ok(denoms) => denoms,
            Err(err) => panic!(
                "Can not get supported denom pairs from oracle contract {:?}",
                err
            ),
        };

        let future = match cfg.continuous {
            true => provider.continuous(&supported_denom_pairs, &cosm_client, cfg.tick_time),
            false => provider.single_run(&supported_denom_pairs, &cosm_client),
        };
        if let Err(err) = rt.block_on(future) {
            panic!("{:?}", err);
        }
    }
    Ok(())
}

fn read_config() -> std::io::Result<Config> {
    let content = std::fs::read_to_string("market-data-feeder.toml")?;
    Ok(toml::from_str(&content)?)
}
