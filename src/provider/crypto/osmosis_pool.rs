use serde::Deserialize;

use crate::provider::{FeedProviderError, Price};

#[derive(Deserialize, Debug, Clone)]
pub struct Token {
    denom: String,
    amount: String,
}

impl Default for Token {
    fn default() -> Self {
        Self {
            denom: String::default(),
            amount: String::from("0"),
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

impl Default for PoolAsset {
    fn default() -> Self {
        Self {
            token: Default::default(),
            weight: String::from("0"),
        }
    }
}

#[derive(Deserialize, Debug, Default)]
pub struct Pool {
    pub address: String,
    pub id: String,
    #[serde(rename = "poolAssets")]
    pool_assets: Vec<PoolAsset>,
}

impl Pool {
    pub fn get_assets_cnt(&self) -> usize {
        self.pool_assets.len()
    }

    pub fn parse_pool_assets_by_denoms(
        &self,
        token_base_denom: &str,
        token_quote_denom: &str,
    ) -> Result<PoolAssetPair, FeedProviderError> {
        let base = self
            .get_pool_asset_by_denom(&self.pool_assets, token_base_denom)
            .cloned()?;

        let quote = self
            .get_pool_asset_by_denom(&self.pool_assets, token_quote_denom)
            .cloned()?;

        Ok(PoolAssetPair { base, quote })
    }

    pub fn spot_price(
        &self,
        base_asset: &str,
        quote_asset: &str,
    ) -> Result<Price, FeedProviderError> {
        // spot_price is calculated with the following formula
        // (Base_supply / Weight_base) / (Quote_supply / Weight_quote)
        // or this is equal to
        // (Base_supply * Weight_quote) / (Quote_supply * Weight_base )
        //
        // Formula taken from here:
        // https://docs.osmosis.zone/developing/osmosis-core/modules/spec-gamm.html#spot-price
        // maybe we can switch to using grpc query about the spot price instead of parcing all available pools
        // see https://github.com/cosmos/cosmos-rust/pull/270 for osmosis proto
        // Also osmosis-std
        // https://github.com/osmosis-labs/osmosis-rust/blob/5da0d5eace1bc39ac49b2f8682bfb3303bc402e6/packages/osmosis-std/src/types/osmosis/gamm/v1beta1.rs#L363

        // TODO check again if weight is needed

        let asset_pair = self.parse_pool_assets_by_denoms(base_asset, quote_asset)?;

        if asset_pair.base.weight.is_empty() || asset_pair.quote.weight.is_empty() {
            return Err(FeedProviderError::InvalidPoolEmptyWeight);
        }

        // TODO avoid unchecked multiply
        Ok(Price::new(
            base_asset,
            asset_pair.get_base_amount() * asset_pair.get_quote_weight(),
            quote_asset,
            asset_pair.get_quote_amount() * asset_pair.get_base_weight(),
        ))
    }

    fn get_pool_asset_by_denom<'r>(
        &self,
        assets: &'r [PoolAsset],
        denom: &str,
    ) -> Result<&'r PoolAsset, FeedProviderError> {
        assets
            .iter()
            .find(|pool| pool.token.denom == denom)
            .ok_or_else(|| FeedProviderError::DenomNotFound {
                denom: String::from(denom),
            })
    }
}

#[cfg(test)]
mod tests {
    use crate::provider::{
        crypto::osmosis_pool::{Pool, PoolAsset, Token},
        Price,
    };

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
        let asset = &pool
            .get_pool_asset_by_denom(&pool.pool_assets, "uosmo")
            .cloned()
            .unwrap();

        assert_eq!(asset.weight, "536870912000000".to_string());

        let price = pool
            .spot_price(
                "ibc/27394FB092D2ECCD56123C74F36E4C1F926001CEADA9CA97EA622B25F41E5EB2",
                "uosmo",
            )
            .unwrap();

        assert_eq!(
            price,
            Price::new(
                "ibc/27394FB092D2ECCD56123C74F36E4C1F926001CEADA9CA97EA622B25F41E5EB2",
                3702798680064000000,
                "uosmo",
                15275051188224000000,
            )
        )
    }
}
