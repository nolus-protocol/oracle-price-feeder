use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct Config {
    pub continuous: bool,
    pub tick_time: u64,
    pub providers: Vec<Providers>,
    pub oracle: Oracle,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Providers {
    pub main_type: String,
    pub name: String,
    pub base_address: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct Oracle {
    pub(crate) contract_addrs: String,
    pub(crate) host_url: String,
    pub(crate) rpc_port: u16,
    pub(crate) grpc_port: u16,
    pub(crate) lcd_port: u16,
    pub(crate) prefix: String,
    pub(crate) chain_id: String,
    pub(crate) fee_denom: String,
    pub(crate) funds_amount: u32,
    pub(crate) gas_limit: u64,
}

impl Oracle {
    pub fn create(contract_addrs: String) -> OracleBuilder {
        OracleBuilder {
            contract_addrs,
            host_url: "http://localhost".to_string(),
            rpc_port: 26612,
            grpc_port: 26615,
            lcd_port: 26614,
            prefix: "nolus".to_string(),
            chain_id: "nolus-local".to_string(),
            fee_denom: "unls".to_string(),
            funds_amount: 2500u32,
            gas_limit: 500_000,
        }
    }
}

pub struct OracleBuilder {
    contract_addrs: String,
    host_url: String,
    rpc_port: u16,
    grpc_port: u16,
    lcd_port: u16,
    prefix: String,
    chain_id: String,
    fee_denom: String,
    funds_amount: u32,
    gas_limit: u64,
}

impl OracleBuilder {
    pub fn host_url(&mut self, host_url: &str) -> &mut Self {
        self.host_url = host_url.to_owned();
        self
    }
    pub fn rpc_port(&mut self, rpc_port: u16) -> &mut Self {
        self.rpc_port = rpc_port;
        self
    }
    pub fn grpc_port(&mut self, grpc_port: u16) -> &mut Self {
        self.grpc_port = grpc_port;
        self
    }
    pub fn lcd_port(&mut self, lcd_port: u16) -> &mut Self {
        self.lcd_port = lcd_port;
        self
    }

    pub fn build(&self) -> Oracle {
        Oracle {
            contract_addrs: self.contract_addrs.clone(),
            host_url: self.host_url.clone(),
            rpc_port: self.rpc_port,
            grpc_port: self.grpc_port,
            lcd_port: self.lcd_port,
            prefix: self.prefix.clone(),
            chain_id: self.chain_id.clone(),
            fee_denom: self.fee_denom.clone(),
            funds_amount: self.funds_amount,
            gas_limit: self.gas_limit,
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            continuous: false,
            tick_time: 5,
            providers: vec![],
            oracle: Oracle::create("".to_string()).build(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Config;

    #[test]
    fn default_config() {
        let cfg = Config::default();
        assert_eq!("nolus".to_string(), cfg.oracle.prefix);
        assert_eq!("nolus-local".to_string(), cfg.oracle.chain_id);
        assert_eq!("unls".to_string(), cfg.oracle.fee_denom);
        assert_eq!(500_000, cfg.oracle.gas_limit);
        assert_eq!(2500u32, cfg.oracle.funds_amount);
        assert!(!cfg.continuous);
    }
}
