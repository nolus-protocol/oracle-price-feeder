use crate::{
    configuration::Oracle,
    cosmos::{
        client::Client,
        QueryMsg,
        SupportedDenomPairsResponse,
    },
};

use super::ORACLE_ADDRESS;

#[actix_rt::test]
async fn get_account_data_example() {
    let config = Oracle::create(ToOwned::to_owned(ORACLE_ADDRESS))
        .host_url("https://net-dev.nolus.io")
        .grpc_port(26625)
        .build();
    let client = Client::new(config.clone()).unwrap();

    let account = client
        .get_account_data(&config.contract_addrs)
        .await
        .unwrap();
    assert_eq!(account.account_number, 15);
    assert_eq!(account.address, ToOwned::to_owned(ORACLE_ADDRESS));
}

#[actix_rt::test]
async fn get_supported_denom_pairs() {
    let config = Oracle::create(ToOwned::to_owned(ORACLE_ADDRESS))
        .host_url("https://net-dev.nolus.io")
        .grpc_port(26625)
        .build();
    let client = Client::new(config).unwrap();

    let response = client
        .cosmwasm_query(&QueryMsg::SupportedCurrencyPairs {})
        .await
        .unwrap();
    let pairs: SupportedDenomPairsResponse = serde_json::from_slice(&response.data).unwrap();
    println!("{:?}", pairs);
}

#[actix_rt::test]
async fn get_account_data_example_dev() {
    // https://github.com/hyperium/tonic/issues/240
    // https://github.com/hyperium/tonic/issues/643

    let config = Oracle::create(ToOwned::to_owned(ORACLE_ADDRESS))
        .host_url("https://net-dev.nolus.io")
        .grpc_port(26625)
        .build();

    let cosmos_client = Client::new(config).unwrap();

    let account = cosmos_client
        .get_account_data(ORACLE_ADDRESS)
        .await
        .unwrap();
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
