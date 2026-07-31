use std::{collections::HashMap, io, path::PathBuf, time::Duration};

use thiserror::Error;
use tokio::task::JoinHandle;
use xframe::{
    Application, ApplicationResult, DiscoveryConfig, FrameConfig, FrameHandle, NodeConfig,
    RpcConfig, ServiceType, ShutdownConfig,
};

use crate::mail::{MailService, MailSettings};

pub use xkk_config::PublicConfig as Config;

#[derive(Debug, Error)]
pub enum ServiceError {
    #[error(transparent)]
    Config(#[from] xkk_config::ConfigError),
    #[error(transparent)]
    Frame(#[from] xframe::Error),
    #[error(transparent)]
    Log(#[from] xlog::Error),
    #[error("close Public log worker: {0}")]
    LogClose(#[source] io::Error),
    #[error(transparent)]
    Mongo(#[from] xframe::xmongo::Error),
    #[error(transparent)]
    Redis(#[from] xframe::xredis::Error),
    #[error(transparent)]
    Rpc(#[from] xframe::xrpc::Error),
    #[error(transparent)]
    Protocol(#[from] xkk_protocol::ProtocolError),
}

fn frame_config(config: &Config) -> Result<FrameConfig, ServiceError> {
    let metadata = HashMap::from([("protocol".to_string(), "ss".to_string())]);
    let node = NodeConfig::new(
        &config.node.cluster,
        ServiceType::Public,
        config.node.instance_id,
        &config.node.advertise_host,
        config.node.service_port,
    )?
    .with_versions(config.node.pro_version, config.node.conf_version)
    .with_meta_data(metadata);
    let discovery = DiscoveryConfig::new(&config.infrastructure.etcd_dsn)?.with_lease_ttl(
        Duration::from_secs(config.infrastructure.etcd_lease_ttl_seconds),
    )?;
    let listener = xframe::xnet::ServerConfig::with_listener(xframe::xnet::ListenEndpoint::tcp(
        format!("{}:{}", config.node.listen_host, config.node.service_port),
    ))
    .with_transport(
        xframe::xnet::TransportOptions::default()
            .with_write_queue_capacity(config.capacity.write_queue),
    );

    Ok(FrameConfig::new(node)
        .with_discovery(discovery)
        .with_service_server(listener)
        .with_mongo(xframe::xmongo::Config::new(
            &config.infrastructure.mongo_dsn,
        )?)
        .with_redis(xframe::xredis::RedisConfig::new(
            &config.infrastructure.redis_dsn,
        )?)
        .with_rpc(RpcConfig::new(config.capacity.rpc_pending)?)
        .with_shutdown(ShutdownConfig::new(Duration::from_secs(
            config.runtime.shutdown_drain_seconds,
        ))?))
}

pub fn config_path() -> Result<PathBuf, ServiceError> {
    Ok(xkk_config::config_path("public")?)
}

pub async fn run(config: Config) -> Result<(), ServiceError> {
    config.validate()?;
    let log_options = config.log.options("logs/public.log");
    let frame_config = frame_config(&config)?;
    let cluster = config.node.cluster.clone();
    let metrics_interval = Duration::from_secs(config.runtime.metrics_interval_seconds);
    let instance_id = config.node.instance_id;
    let rpc_timeout = Duration::from_millis(config.runtime.rpc_timeout_ms);
    let mail_lock_ttl = Duration::from_secs(config.runtime.mail_lock_seconds);
    let log_guard = xlog::init_global(log_options)?;

    let service: Result<(), ServiceError> = async {
        xkk_protocol::init_global_registry()?;
        let prepared = xframe::prepare(frame_config).await?;
        let handle = prepared.handle();
        let mongo = handle
            .mongo()
            .expect("Public FrameConfig always enables Mongo");
        let redis = handle
            .redis()
            .expect("Public FrameConfig always enables Redis");
        let collection = mongo.collection(
            &config.storage.mongo_database,
            &config.storage.mail_collection,
        );
        let mail = MailService::new(
            handle,
            redis,
            collection,
            MailSettings {
                max_mails: config.capacity.max_mails_per_player,
                rpc_timeout,
                lock_ttl: mail_lock_ttl,
            },
        );
        mail.register_handlers(prepared.rpc())?;
        let frame = prepared
            .start(PublicApplication::new(cluster, metrics_interval))
            .await?;
        tracing::info!(instance_id, "Public service started");
        let shutdown = frame.run_until_shutdown_signal().await;
        tracing::info!(
            instance_id,
            success = shutdown.is_ok(),
            "Public service stopped"
        );
        shutdown?;
        Ok(())
    }
    .await;
    let log_close = log_guard.close().await;
    service?;
    log_close.map_err(ServiceError::LogClose)?;
    Ok(())
}

struct PublicApplication {
    cluster: String,
    metrics_interval: Duration,
    metrics_task: Option<JoinHandle<()>>,
}

impl PublicApplication {
    fn new(cluster: String, metrics_interval: Duration) -> Self {
        Self {
            cluster,
            metrics_interval,
            metrics_task: None,
        }
    }
}

impl Application for PublicApplication {
    async fn start(&mut self, frame: FrameHandle) -> ApplicationResult {
        frame.watch(self.cluster.clone(), ServiceType::Gate).await?;
        frame
            .watch(self.cluster.clone(), ServiceType::Logic)
            .await?;
        self.metrics_task = spawn_metrics(frame, self.metrics_interval);
        Ok(())
    }

    async fn shutdown(&mut self, _frame: FrameHandle) -> ApplicationResult {
        if let Some(task) = self.metrics_task.take() {
            task.abort();
            let _ = task.await;
        }
        Ok(())
    }
}

fn spawn_metrics(frame: FrameHandle, interval: Duration) -> Option<JoinHandle<()>> {
    if interval.is_zero() {
        return None;
    }
    Some(tokio::spawn(async move {
        let mut ticker = tokio::time::interval(interval);
        ticker.tick().await;
        loop {
            ticker.tick().await;
            let stats = frame.stats();
            tracing::info!(
                frame_state = ?stats.state,
                active_sessions = stats.sessions.active_sessions,
                write_queue_depth = stats.sessions.outbound_queue_depth,
                rpc_pending = stats.rpc.pending,
                rpc_inbound_active = stats.rpc.inbound_active,
                rpc_pending_rejected = stats.rpc.pending_rejected,
                listeners = ?stats.listeners,
                "Public runtime stats"
            );
        }
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn example_config_builds_internal_service_listener() {
        let config = Config::parse(include_str!("../../../config/public.yaml")).unwrap();
        let frame = frame_config(&config).unwrap();

        assert!(frame.service_server.is_some());
        assert!(frame.http.is_none());
        assert_eq!(frame.node.meta_data().get("protocol").unwrap(), "ss");
    }
}
