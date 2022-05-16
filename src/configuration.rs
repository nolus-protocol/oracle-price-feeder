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

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Oracle {
    pub contract_addrs: String,
    pub cosmos_url: String,
    pub prefix: String,
    pub chain_id: String,
    pub fee_denom: String,
    pub funds_amount: u32,
    pub gas_limit: u64,
}

impl ::std::default::Default for Config {
    fn default() -> Self {
        Self {
            continuous: false,
            tick_time: 5,
            providers: vec![],
            oracle: Oracle {
                contract_addrs: "".to_string(),
                cosmos_url: "".to_string(),
                prefix: "nolus".to_string(),
                chain_id: "nolus-local".to_string(),
                fee_denom: "unolus".to_string(),
                funds_amount: 100u32,
                gas_limit: 500_000,
            },
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
        assert_eq!("unolus".to_string(), cfg.oracle.fee_denom);
        assert_eq!(500_000, cfg.oracle.gas_limit);
        assert_eq!(100u32, cfg.oracle.funds_amount);
        assert!(!cfg.continuous);
    }
}
