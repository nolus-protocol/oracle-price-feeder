use chain_comms::{
    client::Client,
    config::Node,
    interact::{query_account_data, query_wasm},
};

use crate::messages::{QueryMsg, SupportedCurrencyPairsResponse};

use super::{NODE_CONFIG, ORACLE_ADDRESS};

#[actix_rt::test]
async fn get_account_data_example() {
    let config = toml::from_str::<Node>(NODE_CONFIG).unwrap();
    let client = Client::new(&config).await.unwrap();

    let account = query_account_data(&client, ORACLE_ADDRESS).await.unwrap();
    assert_eq!(account.account_number, 15);
    assert_eq!(account.address, ToOwned::to_owned(ORACLE_ADDRESS));
}

#[actix_rt::test]
async fn get_supported_denom_pairs() {
    let config = toml::from_str::<Node>(NODE_CONFIG).unwrap();
    let client = Client::new(&config).await.unwrap();

    let pairs: SupportedCurrencyPairsResponse = query_wasm(
        &client,
        ORACLE_ADDRESS,
        &serde_json_wasm::to_vec(&QueryMsg::SupportedCurrencyPairs {}).unwrap(),
    )
    .await
    .unwrap();
    println!("{:?}", pairs);
}

#[actix_rt::test]
async fn get_account_data_example_dev() {
    // https://github.com/hyperium/tonic/issues/240
    // https://github.com/hyperium/tonic/issues/643

    let config = toml::from_str::<Node>(NODE_CONFIG).unwrap();

    let client = Client::new(&config).await.unwrap();

    let account = query_account_data(&client, ORACLE_ADDRESS).await.unwrap();
    assert_eq!(account.account_number, 15);
}

// // TODO: move to integration test = > start network with docker and exec transaction there
// #[actix_rt::test]
// async fn send_transaction() {
//     const DEFAULT_COSMOS_HD_PATH: &str = "m/44'/118'/0'/0/0";
//     const TEST_MNEMONIC: &str = "agent repeat phrase universe happy depart defense visit visa one cricket three mother slim knock school problem amateur trick embrace bracket hill arena evidence";
//
//     let config = Oracle::create(
//         ToOwned::to_owned(ORACLE_ADDRESS),
//     )
//         .build();
//     let price_feed_json = json!({
//         "feed_prices": {
//             "prices": [{"base":"OSMO", "values" : [{"denom": "uusdc", "amount": "1.2"}]}]
//         }
//     });
//
//     let wallet = Wallet::new(TEST_MNEMONIC, DEFAULT_COSMOS_HD_PATH).unwrap();
//     let client = CosmosClient::new(config.clone()).unwrap();
//
//     broadcast_tx(&client, &wallet, &config, price_feed_json.to_string())
//         .await
//         .unwrap();
// }
