use crate::provider::FeedProviderError;
use cosmwasm_std::{Decimal256, Uint256};
use serde::Deserialize;

trait Empty<T> {
    fn empty() -> T;
}

#[derive(Deserialize, Debug, Clone)]
pub struct Token {
    denom: String,
    amount: String,
}

impl Empty<Token> for Token {
    fn empty() -> Token {
        Token {
            denom: "".to_string(),
            amount: "0".to_string(),
        }
    }
}

#[derive(Deserialize, Debug, Clone)]
pub struct PoolAssetPair {
    base: PoolAsset,
    quote: PoolAsset,
}

impl PoolAssetPair {
    fn get_base_amount(&self) -> u128 {
        self.base.token.amount.parse::<u128>().unwrap_or_default()
    }
    fn get_quote_amount(&self) -> u128 {
        self.quote.token.amount.parse::<u128>().unwrap_or_default()
    }
    fn get_base_weight(&self) -> u128 {
        self.base.weight.parse::<u128>().unwrap_or_default()
    }
    fn get_quote_weight(&self) -> u128 {
        self.quote.weight.parse::<u128>().unwrap_or_default()
    }
}

#[derive(Deserialize, Debug, Clone)]
pub struct PoolAsset {
    token: Token,
    weight: String,
}

impl Empty<PoolAsset> for PoolAsset {
    fn empty() -> PoolAsset {
        PoolAsset {
            token: Token::empty(),
            weight: "0".to_string(),
        }
    }
}

#[derive(Deserialize, Debug)]
pub struct Pool {
    pub address: String,
    pub id: String,
    #[serde(rename = "poolAssets")]
    pool_assets: Vec<PoolAsset>,
}

impl Empty<Pool> for Pool {
    fn empty() -> Pool {
        Pool {
            address: "".to_string(),
            id: "".to_string(),
            pool_assets: vec![],
        }
    }
}

impl Pool {
    pub fn get_assets_cnt(&self) -> usize {
        self.pool_assets.len()
    }

    pub fn parse_pool_assets_by_denoms(
        &self,
        token_adenom: &str,
        token_bdenom: &str,
    ) -> Result<PoolAssetPair, FeedProviderError> {
        let (a_asset, found) = self.get_pool_asset_by_denom(self.pool_assets.clone(), token_adenom);
        if !found {
            return Err(FeedProviderError::DenomNotFound {
                denom: token_adenom.to_string(),
            });
        }
        let (b_asset, found) = self.get_pool_asset_by_denom(self.pool_assets.clone(), token_bdenom);
        if !found {
            return Err(FeedProviderError::DenomNotFound {
                denom: token_bdenom.to_string(),
            });
        }
        Ok(PoolAssetPair {
            base: a_asset,
            quote: b_asset,
        })
    }

    fn get_pool_asset_by_denom(&self, assets: Vec<PoolAsset>, denom: &str) -> (PoolAsset, bool) {
        for asset in assets {
            if asset.token.denom == denom {
                return (asset, true);
            }
        }
        (PoolAsset::empty(), false)
    }

    pub fn spot_price(
        &self,
        base_asset: &str,
        quote_asset: &str,
    ) -> Result<Decimal256, FeedProviderError> {
        // spot_price is calculated with the following formula
        // (Base_supply / Weight_base) / (Quote_supply / Weight_quote)

        let asset_pair = self.parse_pool_assets_by_denoms(base_asset, quote_asset)?;
        if asset_pair.base.weight.is_empty() || asset_pair.quote.weight.is_empty() {
            return Err(FeedProviderError::InvalidPoolEmptyWeight);
        }

        let numerator =
            Decimal256::from_ratio(asset_pair.get_base_amount(), asset_pair.get_base_weight());
        let denom =
            Decimal256::from_ratio(asset_pair.get_quote_amount(), asset_pair.get_quote_weight());
        let ratio = decimal_div(numerator, denom);
        Ok(ratio)
    }
}

fn decimal_div(numerator: Decimal256, denominator: Decimal256) -> Decimal256 {
    let decimal_fractional: Uint256 = // 1*10**18
        Uint256::from_be_bytes([
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 13, 224, 182,
            179, 167, 100, 0, 0,
        ]);
    Decimal256::from_ratio(
        numerator * decimal_fractional,
        denominator * decimal_fractional,
    )
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use cosmwasm_std::Decimal256;

    use crate::provider::crypto::osmosis_pool::{Pool, PoolAsset, Token};

    use super::decimal_div;

    #[tokio::test]
    async fn get_spot_price() {
        let pool = Pool {
            address: "osmo124qc2hs5jgp2shrmtv2usxyrt52k447702pczyct0zqadlkkh2csvh5pzv".to_string(),
            id: "97".to_string(),
            pool_assets: vec![
                PoolAsset {
                    token: Token {
                        denom:
                            "ibc/27394FB092D2ECCD56123C74F36E4C1F926001CEADA9CA97EA622B25F41E5EB2"
                                .to_string(),
                        amount: "6897".to_string(),
                    },
                    weight: "536870912000000".to_string(),
                },
                PoolAsset {
                    token: Token {
                        denom: "uosmo".to_string(),
                        amount: "28452".to_string(),
                    },
                    weight: "536870912000000".to_string(),
                },
            ],
        };

        // Assert
        let asset = &pool.get_pool_asset_by_denom(pool.pool_assets.clone(), "uosmo");

        assert!(asset.1);
        assert_eq!(asset.0.weight, "536870912000000".to_string());

        let price = pool
            .spot_price(
                "ibc/27394FB092D2ECCD56123C74F36E4C1F926001CEADA9CA97EA622B25F41E5EB2",
                "uosmo",
            )
            .unwrap();

        assert_eq!("0.242408258936022297".to_string(), price.to_string())
    }

    #[test]
    fn test_decimal_division() {
        let nominator = Decimal256::from_str("4").unwrap();
        let denominator = Decimal256::from_str("2").unwrap();

        let ratio = decimal_div(nominator, denominator);
        assert_eq!("2", ratio.to_string());

        let nominator = Decimal256::from_str("2").unwrap();
        let denominator = Decimal256::from_str("4").unwrap();
        let ratio = decimal_div(nominator, denominator);
        assert_eq!("0.5", ratio.to_string());

        let nominator = Decimal256::from_str("10.10").unwrap();
        let denominator = Decimal256::from_str("5.5").unwrap();
        let ratio = decimal_div(nominator, denominator);
        assert_eq!("1.836363636363636363", ratio.to_string());

        let nominator = Decimal256::from_str("0.000000000124456789").unwrap();
        let denominator = Decimal256::from_str("5.65").unwrap();
        let ratio = decimal_div(nominator, denominator);
        assert_eq!("0.00000000002202775", ratio.to_string());

        let nominator = Decimal256::from_str("0.000000000124456789").unwrap();
        let denominator = Decimal256::from_str("0.000000000856756789").unwrap();
        let ratio = decimal_div(nominator, denominator);
        assert_eq!("0.145265016394285029", ratio.to_string());
    }
}
