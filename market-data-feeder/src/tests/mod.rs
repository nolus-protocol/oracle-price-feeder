#![cfg(test)]

// pub mod integration;
pub mod integration_dev_network;

const ORACLE_ADDRESS: &str = "nolus1436kxs0w2es6xlqpp9rd35e3d0cjnw4sv8j3a7483sgks29jqwgsv3wzl4";

const NODE_CONFIG: &str = r#"address_prefix = "nolus"
chain_id = "nolus-dev-v0-1-42"
gas_adjustment_numerator = 105
gas_adjustment_denominator = 100
fee_denom = "unls"
gas_price_numerator = 1
gas_price_denominator = 390"#;
