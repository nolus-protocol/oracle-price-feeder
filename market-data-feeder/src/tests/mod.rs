#![cfg(test)]

// pub mod integration;
pub mod integration_dev_network;

const ORACLE_ADDRESS: &str = "nolus1436kxs0w2es6xlqpp9rd35e3d0cjnw4sv8j3a7483sgks29jqwgsv3wzl4";

const NODE_CONFIG: &str = r#"#http2_concurrency_limit = 1
json_rpc_protocol = "https"
grpc_protocol = "https"
json_rpc_host = "net-dev.nolus.io"
grpc_host = "net-dev.nolus.io"
json_rpc_port = 26617
# json_rpc_api_path = "/json-rpc/api"
grpc_port = 26620
# grpc_api_path = "/grpc/api"
address_prefix = "nolus"
chain_id = "nolus-dev-v0-1-42"
gas_adjustment_numerator = 105
gas_adjustment_denominator = 100
fee_denom = "unls"
gas_price_numerator = 1
gas_price_denominator = 390"#;
