use std::time::Duration;

use cosmrs::{
    proto::cosmos::base::tendermint::v1beta1::service_client::ServiceClient as TendermintServiceClient,
    tendermint::block::Height,
};
use tonic::transport::Channel as TonicChannel;

use super::query;

pub mod error;

pub struct Healthcheck {
    service_client: TendermintServiceClient<TonicChannel>,
    last_height: Height,
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
            })
            .map_err(error::Construct::LatestBlockHeight)
    }

    pub async fn check(&mut self) -> Result<(), error::Error> {
        Self::check_sync(&mut self.service_client).await?;

        Self::get_height(&mut self.service_client)
            .await
            .map_err(error::Error::LatestBlockHeight)
            .and_then(|height| {
                if height > self.last_height {
                    self.last_height = height;

                    Ok(())
                } else {
                    Err(error::Error::BlockHeightNotIncremented)
                }
            })
    }

    pub async fn wait_until_healthy<NotHealthyF, HealthyF>(
        &mut self,
        mut not_healthy: NotHealthyF,
        healthy: HealthyF,
    ) -> Result<(), error::Error>
    where
        NotHealthyF: FnMut(WaitUntilHealthyStatusType) + Send,
        HealthyF: FnOnce() + Send,
    {
        while let Err(error) = self.check().await {
            match error {
                error::Error::Syncing(error::CheckSyncing::Syncing) => {
                    not_healthy(WaitUntilHealthyStatusType::Syncing);
                },
                error::Error::BlockHeightNotIncremented => {
                    not_healthy(
                        WaitUntilHealthyStatusType::BlockNotIncremented,
                    );
                },
                _ => return Err(error),
            }

            tokio::time::sleep(Duration::from_secs(1)).await;
        }

        healthy();

        Ok(())
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
    ) -> Result<Height, error::LatestBlockHeight> {
        query::latest_block(service_client)
            .await?
            .header
            .ok_or(error::LatestBlockHeight::NoBlockHeaderReturned)
            .and_then(|header| header.height.try_into().map_err(From::from))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WaitUntilHealthyStatusType {
    Syncing,
    BlockNotIncremented,
}
