use std::{future::Future, io};

use thiserror::Error;
use xkk_config::Infrastructure;

#[derive(Debug, Error)]
pub enum Error {
    #[error(transparent)]
    Log(#[from] xlog::Error),
    #[error("close process log worker: {0}")]
    LogClose(#[source] io::Error),
    #[error(transparent)]
    Protocol(#[from] xkk_protocol::ProtocolError),
    #[error(transparent)]
    Mongo(#[from] xmongo::Error),
    #[error(transparent)]
    Redis(#[from] xredis::Error),
}

/// Shared clients only; each service owns its xframe composition and business state.
#[derive(Clone)]
pub struct Resources {
    pub mongo: xmongo::Client,
    pub redis: xredis::Client,
}

impl Resources {
    async fn connect(config: &Infrastructure) -> Result<Self, Error> {
        // Validate both client configurations before opening either connection.
        let redis_config = xredis::RedisConfig::new(&config.redis_dsn)?;
        let mongo_config = xmongo::Config::new(&config.mongo_dsn)?;
        let redis = xredis::Client::connect_config(redis_config).await?;
        let mongo = match xmongo::Client::connect_config(mongo_config).await {
            Ok(mongo) => mongo,
            Err(error) => {
                redis.close();
                return Err(error.into());
            }
        };
        Ok(Self { mongo, redis })
    }

    async fn close(self) {
        self.mongo.shutdown().await;
        self.redis.close();
    }
}

/// Runs one service with shared process setup and reverse-order resource cleanup.
/// The service future must await its frame shutdown before returning. No global
/// resource accessor or business lifecycle hooks are introduced here.
pub async fn run<F, Fut, E>(log: xlog::Options, infrastructure: &Infrastructure, service: F) -> Result<(), E>
where
    F: FnOnce(Resources) -> Fut,
    Fut: Future<Output = Result<(), E>>,
    E: From<Error>,
{
    let log_guard = xlog::init_global(log).map_err(Error::from)?;
    let result: Result<(), E> = async {
        xkk_common::service_type::init();
        xkk_protocol::init_global_registry().map_err(Error::from)?;
        let resources = Resources::connect(infrastructure).await?;
        let result = service(resources.clone()).await;
        resources.close().await;
        result
    }
    .await;
    let log_close = log_guard.close().await;
    if result.is_err() {
        if let Err(error) = log_close {
            eprintln!("close process log worker after service failure: {error}");
        }
        return result;
    }
    log_close.map_err(Error::LogClose)?;
    Ok(())
}
