use std::time::Duration;

use cosmrs::{
    proto::cosmos::base::tendermint::v1beta1::service_client::ServiceClient as TendermintServiceClient,
    tendermint::block::Height,
};
use tokio::time::Instant;
use tonic::transport::Channel as TonicChannel;

use super::query;

pub mod error;

pub struct Healthcheck {
    service_client: TendermintServiceClient<TonicChannel>,
    last_height: Height,
    last_checked: Instant,
}

impl Healthcheck {
    pub async fn new(
        mut service_client: TendermintServiceClient<TonicChannel>,
    ) -> Result<Self, error::Construct> {
        Self::check_sync(&mut service_client).await?;

        Self::get_height(&mut service_client)
            .await
            .map(|last_height| Self {
                service_client,
                last_height,
                last_checked: Instant::now(),
            })
            .map_err(error::Construct::LatestBlockHeight)
    }

    pub async fn check(&mut self) -> Result<(), error::Error> {
        if self.last_checked.elapsed() < Duration::from_secs(5) {
            return Ok(());
        }

        Self::check_sync(&mut self.service_client).await?;

        Self::get_height(&mut self.service_client)
            .await
            .map_err(error::Error::LatestBlockHeight)
            .and_then(|height| {
                if height > self.last_height {
                    self.last_height = height;

                    self.last_checked = Instant::now();

                    Ok(())
                } else {
                    Err(error::Error::BlockHeightNotIncremented)
                }
            })
    }

    async fn check_sync(
        service_client: &mut TendermintServiceClient<TonicChannel>,
    ) -> Result<(), error::CheckSyncing> {
        query::syncing(service_client)
            .await
            .map_err(From::from)
            .and_then(|syncing| {
                if syncing {
                    Err(error::CheckSyncing::Syncing)
                } else {
                    Ok(())
                }
            })
    }

    async fn get_height(
        service_client: &mut TendermintServiceClient<TonicChannel>,
    ) -> Result<Height, error::GetHeight> {
        query::latest_block(service_client)
            .await?
            .header
            .ok_or(error::GetHeight::NoBlockHeaderReturned)
            .and_then(|header| header.height.try_into().map_err(From::from))
    }
}
