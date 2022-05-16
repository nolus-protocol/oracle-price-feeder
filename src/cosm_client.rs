use cosmos_sdk_proto::cosmos::tx::v1beta1::BroadcastMode;
use cosmos_sdk_proto::cosmwasm::wasm::v1::query_client::QueryClient as WasmQueryClient;
use cosmos_sdk_proto::cosmwasm::wasm::v1::MsgExecuteContract;
use cosmos_sdk_proto::cosmwasm::wasm::v1::QuerySmartContractStateRequest;
use cosmos_sdk_proto::cosmwasm::wasm::v1::QuerySmartContractStateResponse;
use deep_space::client::ChainStatus;
use deep_space::Address;
use deep_space::Contact;
use deep_space::Fee;
use deep_space::Msg;
use deep_space::PrivateKey;
use deep_space::{Coin, MessageArgs};
use std::time::Duration;

use crate::configuration::Oracle;
use crate::errors::FeederError;

const TIMEOUT: Duration = Duration::from_secs(60);
const EXEC_MSG_TYPE_URL: &str = "/cosmwasm.wasm.v1.MsgExecuteContract";

pub struct CosmClient {
    config: Oracle,

    sender_address: Address,
    private_key: PrivateKey,
}

impl CosmClient {
    pub fn new(cfg: &Oracle, secret: String) -> Result<CosmClient, FeederError> {
        // validate contract address
        if let Err(err) = bech32::decode(cfg.contract_addrs.as_str()) {
            eprintln!("{:?}", err);
            return Err(FeederError::InvalidOracleContractAddress {
                address: cfg.contract_addrs.clone(),
            });
        };

        let (addr, key) = match CosmClient::prepare_keys(cfg, secret) {
            Ok((a, k)) => (a, k),
            Err(err) => {
                eprintln!("{:?}", err);
                return Err(FeederError::AuthError {
                    message: err.to_string(),
                });
            }
        };
        Ok(CosmClient {
            sender_address: addr,
            private_key: key,
            config: cfg.clone(),
        })
    }

    fn prepare_keys(
        cfg: &Oracle,
        secret: String,
    ) -> Result<(Address, PrivateKey), Box<dyn std::error::Error>> {
        let private_key = PrivateKey::from_phrase(&secret, "")?;
        let public_key = private_key.to_public_key(&cfg.prefix)?;
        Ok((public_key.to_address(), private_key))
    }

    pub async fn generate_and_send_tx(
        &self,
        json_msg: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let contact = Contact::new(&self.config.cosmos_url, TIMEOUT, &self.config.prefix)?;

        let mut block_timeout = 100;
        let chain_status = contact.get_chain_status().await?;
        match chain_status {
            ChainStatus::Moving { block_height: h } => block_timeout += h,
            ChainStatus::Syncing | ChainStatus::WaitingToStart => panic!("Chain not running"),
        }

        let coin = Coin {
            denom: self.config.fee_denom.to_string(),
            amount: self.config.funds_amount.into(),
        };

        let exec = MsgExecuteContract {
            sender: self.sender_address.to_string(),
            contract: self.config.contract_addrs.to_string(),
            msg: json_msg.as_bytes().to_vec(),
            funds: vec![coin.clone().into()],
        };

        let fee = Fee {
            amount: vec![coin],
            gas_limit: self.config.gas_limit,
            granter: None,
            payer: None,
        };

        let account_info = contact.get_account_info(self.sender_address).await?;
        let args = MessageArgs {
            sequence: account_info.sequence,
            account_number: account_info.account_number,
            chain_id: self.config.chain_id.to_string(),
            fee,
            timeout_height: block_timeout,
        };

        let msg = Msg::new(EXEC_MSG_TYPE_URL, exec);
        let tx = self.private_key.sign_std_msg(&[msg], args, "")?;
        let tx_resp = contact.send_transaction(tx, BroadcastMode::Sync).await?;

        println!();
        println!("{:?}", tx_resp);

        Ok(())
    }

    pub async fn query_message(
        &self,
        json_msg: &str,
    ) -> Result<QuerySmartContractStateResponse, Box<dyn std::error::Error>> {
        let mut grpc = WasmQueryClient::connect(self.config.cosmos_url.clone())
            .await?
            .accept_gzip();
        Ok(grpc
            .smart_contract_state(QuerySmartContractStateRequest {
                address: self.config.contract_addrs.to_string(),
                query_data: json_msg.as_bytes().to_vec(),
            })
            .await?
            .into_inner())
    }
}

#[cfg(test)]
mod tests {
    use super::CosmClient;
    use crate::configuration::{Config, Oracle};

    const TEST_MNEMONIC: &str = "tide path volcano surface arctic obvious rifle energy nest blame inch solar aerobic debris elegant sunny climb stem extra unique earth present shop brass";
    const TEST_WALLET_ADDRESS: &str = "nolus1hy38we807uawv5j0w3ez5xdfm6elj7l6zzu5ju";
    const TEST_CONTRACT_ADDRESS: &str =
        "nolus17p9rzwnnfxcjp32un9ug7yhhzgtkhvl9jfksztgw5uh69wac2pgsmc5xhq";
    const TEST_COSMOS_URL: &str = "http://localhost:9090";

    fn get_test_config() -> Oracle {
        let mut oracle = Config::default().oracle;
        oracle.contract_addrs = TEST_CONTRACT_ADDRESS.to_string();
        oracle.cosmos_url = TEST_COSMOS_URL.to_string();
        oracle
    }

    #[test]
    fn new_client() {
        let cosm_client = CosmClient::new(&get_test_config(), TEST_MNEMONIC.to_string()).unwrap();
        assert_eq!(TEST_WALLET_ADDRESS, cosm_client.sender_address.to_string());
    }

    #[tokio::test]
    #[should_panic]
    async fn generate_and_send() {
        // Arrange
        let cfg = get_test_config();
        let cosm_client = CosmClient::new(&cfg, TEST_MNEMONIC.to_string()).unwrap();

        // should panic - grpc server not running
        cosm_client
            .generate_and_send_tx(
                r#"{"feed_prices": {
                "prices": []
            }}"#,
            )
            .await
            .unwrap();
    }
}
