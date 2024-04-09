use chain_comms::interact::healthcheck;

#[derive(Debug, thiserror::Error)]
pub enum Error<E>
where
    E: std::error::Error,
{
    #[error("Constructing healthcheck client failed! Error: {0}")]
    HealthcheckConstruct(#[from] healthcheck::error::Construct),
    #[error("Healthcheck failed! Error: {0}")]
    Healthcheck(#[from] healthcheck::error::Error),
    #[error("Spawning generator tasks failed! Error: {0}")]
    SpawnGenerators(E),
}
