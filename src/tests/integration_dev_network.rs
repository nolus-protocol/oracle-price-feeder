use crate::{
    configuration::Oracle,
    cosmos::{client::CosmosClient, QueryMsg},
};

#[actix_rt::test]
async fn get_account_data_example() {
    let config = Oracle::create(
        "nolus1mf6ptkssddfmxvhdx0ech0k03ktp6kf9yk59renau2gvht3nq2gqkxgywu".to_owned(),
    )
    .host_url("https://net-dev.nolus.io")
    .lcd_port(26619)
    .grpc_port(26625)
    .build();
    let client = CosmosClient::new(config.clone()).unwrap();

    let account = client
        .get_account_data(&config.contract_addrs)
        .await
        .unwrap();
    assert_eq!(account.account_number, 16);
    assert_eq!(
        account.address,
        String::from("nolus1mf6ptkssddfmxvhdx0ech0k03ktp6kf9yk59renau2gvht3nq2gqkxgywu")
    );
}

#[actix_rt::test]
async fn get_supported_denom_pairs() {
    let config = Oracle::create(
        "nolus1mf6ptkssddfmxvhdx0ech0k03ktp6kf9yk59renau2gvht3nq2gqkxgywu".to_owned(),
    )
    .host_url("https://net-dev.nolus.io")
    .lcd_port(26619)
    .grpc_port(26625)
    .build();
    let client = CosmosClient::new(config).unwrap();

    let response = client
        .cosmwasm_query(&QueryMsg::SupportedDenomPairs {})
        .await
        .unwrap();
    let pairs: Vec<Vec<String>> = serde_json::from_slice(&response.data).unwrap();
    println!("{:?}", pairs);
}

#[actix_rt::test]
async fn get_account_data_example_dev() {
    let address = "nolus1mf6ptkssddfmxvhdx0ech0k03ktp6kf9yk59renau2gvht3nq2gqkxgywu";

    // https://github.com/hyperium/tonic/issues/240
    // https://github.com/hyperium/tonic/issues/643

    let config = Oracle::create(
        "nolus1mf6ptkssddfmxvhdx0ech0k03ktp6kf9yk59renau2gvht3nq2gqkxgywu".to_owned(),
    )
    .host_url("https://net-dev.nolus.io")
    .lcd_port(26619)
    .grpc_port(26625)
    .build();

    let cosmos_client = CosmosClient::new(config).unwrap();

    let account = cosmos_client.get_account_data(address).await.unwrap();
    assert_eq!(account.account_number, 16);
}

// TODO: move to integration test = > start network with docker and exec transaction there
// #[actix_rt::test]
// async fn send_transaction() {
//     const DEFAULT_COSMOS_HD_PATH: &str = "m/44'/118'/0'/0/0";
//     const TEST_MNEMONIC: &str = "agent repeat phrase universe happy depart defense visit visa one cricket three mother slim knock school problem amateur trick embrace bracket hill arena evidence";

//     let config = Oracle::create(
//         "nolus1mf6ptkssddfmxvhdx0ech0k03ktp6kf9yk59renau2gvht3nq2gqkxgywu".to_owned(),
//     )
//     .build();
//     let price_feed_json = json!({
//         "feed_prices": {
//             "prices": [{"base":"OSMO", "values" : [{"denom": "uusdc", "amount": "1.2"}]}]
//         }
//     });

//     let wallet = Wallet::new(TEST_MNEMONIC, DEFAULT_COSMOS_HD_PATH).unwrap();
//     let client = CosmosClient::new(config.clone()).unwrap();

//     broadcast_tx(&client, &wallet, &config, price_feed_json.to_string())
//         .await
//         .unwrap();
// }
