#![cfg(test)]

// pub mod integration;
pub mod integration_dev_network;

const ORACLE_ADDRESS: &str = "nolus1436kxs0w2es6xlqpp9rd35e3d0cjnw4sv8j3a7483sgks29jqwgsv3wzl4";

const NODE_CONFIG: &str = r#"json_rpc_protocol = "https"
grpc_protocol = "https"
host = "net-dev.nolus.io"
json_rpc_port = 26612
# json_rpc_api_path = "/json-rpc/api"
grpc_port = 26625
# grpc_api_path = "/grpc/api"
address_prefix = "nolus"
chain_id = "nolus-dev-v0-1-42"
fee = { amount = "1500", denom = "unls" }
gas_adjustment_numerator = 1015
gas_adjustment_denominator = 1000"#;
