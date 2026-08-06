use std::{collections::HashMap, io, path::PathBuf, time::Duration};

use thiserror::Error;
use tokio::task::JoinHandle;
use xframe::{
    Application, ApplicationResult, DiscoveryConfig, FrameConfig, FrameHandle, NodeConfig,
    RpcConfig, ServiceType, ShutdownConfig,
};
use xkk_persist::PublicPlayers;

use crate::mail::MailService;

pub use xkk_config::PublicConfig as Config;

// Public's stable process budgets are code-level invariants, not deployment
// knobs. Saturation is surfaced by the metrics task as an error.
const SERVICE_WRITE_QUEUE_CAPACITY: usize = 1_024;
const RPC_PENDING_CAPACITY: usize = 100_000;
const SHUTDOWN_DRAIN_TIMEOUT: Duration = Duration::from_secs(10);
const METRICS_REPORT_INTERVAL: Duration = Duration::from_secs(10);
const PLAYER_SAVE_INTERVAL: Duration = Duration::from_secs(2 * 60);

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
    Persist(#[from] xkk_persist::Error),
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
    .with_versions(config.version.program, config.version.conf)
    .with_meta_data(metadata);
    let discovery = DiscoveryConfig::new(&config.infrastructure.etcd_dsn)?;
    let listener = xframe::xnet::ServerConfig::with_listener(xframe::xnet::ListenEndpoint::tcp(
        format!("{}:{}", config.node.listen_host, config.node.service_port),
    ))
    .with_transport(
        xframe::xnet::TransportOptions::default()
            .with_write_queue_capacity(SERVICE_WRITE_QUEUE_CAPACITY),
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
        .with_rpc(RpcConfig::new(RPC_PENDING_CAPACITY)?)
        .with_shutdown(ShutdownConfig::new(SHUTDOWN_DRAIN_TIMEOUT)?))
}

pub fn config_path() -> Result<PathBuf, ServiceError> {
    Ok(xkk_config::config_path("public")?)
}

pub async fn run(config: Config) -> Result<(), ServiceError> {
    config.validate()?;
    let log_options = config.log.options("logs/public.log");
    let frame_config = frame_config(&config)?;
    let cluster = config.node.cluster.clone();
    let instance_id = config.node.instance_id;
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
        let database = xkk_persist::Database::new(mongo)?;
        let players = PublicPlayers::new(database.public_players());
        let mail = MailService::new(handle, redis, players.clone());
        mail.register_handlers(prepared.rpc())?;
        let frame = prepared
            .start(PublicApplication::new(
                cluster,
                players,
                METRICS_REPORT_INTERVAL,
            ))
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
    players: PublicPlayers,
    metrics_interval: Duration,
    save_task: Option<JoinHandle<()>>,
    metrics_task: Option<JoinHandle<()>>,
}

impl PublicApplication {
    fn new(cluster: String, players: PublicPlayers, metrics_interval: Duration) -> Self {
        Self {
            cluster,
            players,
            metrics_interval,
            save_task: None,
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
        self.save_task = Some(spawn_player_save(
            self.players.clone(),
            PLAYER_SAVE_INTERVAL,
        ));
        self.metrics_task = spawn_metrics(frame, self.players.clone(), self.metrics_interval);
        Ok(())
    }

    async fn shutdown(&mut self, _frame: FrameHandle) -> ApplicationResult {
        stop_task(&mut self.save_task, "Public player save").await;
        match self.players.flush_dirty().await {
            Ok(players) => tracing::info!(players, "Public dirty players flushed at shutdown"),
            Err(error) => {
                tracing::error!(%error, "Public final dirty player flush failed");
            }
        }
        stop_task(&mut self.metrics_task, "Public metrics").await;
        Ok(())
    }
}

fn spawn_player_save(players: PublicPlayers, interval: Duration) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(interval);
        ticker.tick().await;
        loop {
            ticker.tick().await;
            match players.flush_dirty().await {
                Ok(0) => {}
                Ok(count) => tracing::info!(players = count, "Public dirty players flushed"),
                Err(error) => tracing::error!(%error, "Public dirty player flush failed"),
            }
        }
    })
}

async fn stop_task(task: &mut Option<JoinHandle<()>>, name: &'static str) {
    let Some(task) = task.take() else {
        return;
    };
    task.abort();
    if let Err(error) = task.await
        && !error.is_cancelled()
    {
        tracing::error!(task = name, %error, "Public background task failed");
    }
}

fn spawn_metrics(
    frame: FrameHandle,
    players: PublicPlayers,
    interval: Duration,
) -> Option<JoinHandle<()>> {
    if interval.is_zero() {
        return None;
    }
    Some(tokio::spawn(async move {
        let mut ticker = tokio::time::interval(interval);
        let mut last_rpc_pending_rejected = 0;
        let mut last_write_queue_rejected = 0;
        let mut last_player_cache_evictions = 0;
        ticker.tick().await;
        loop {
            ticker.tick().await;
            let stats = frame.stats();
            let player_stats = players.stats();
            let write_queue_rejected = stats.sessions.outbound_rejected_full
                + stats
                    .listeners
                    .iter()
                    .map(|listener| listener.outbound_rejected_full)
                    .sum::<u64>();
            if stats.rpc.pending_rejected > last_rpc_pending_rejected {
                tracing::error!(
                    rejected = stats.rpc.pending_rejected - last_rpc_pending_rejected,
                    total_rejected = stats.rpc.pending_rejected,
                    limit = RPC_PENDING_CAPACITY,
                    "Public RPC pending hard limit exceeded"
                );
                last_rpc_pending_rejected = stats.rpc.pending_rejected;
            }
            if write_queue_rejected > last_write_queue_rejected {
                tracing::error!(
                    rejected = write_queue_rejected - last_write_queue_rejected,
                    total_rejected = write_queue_rejected,
                    limit = SERVICE_WRITE_QUEUE_CAPACITY,
                    "Public write queue hard limit exceeded"
                );
                last_write_queue_rejected = write_queue_rejected;
            }
            if player_stats.cache.evictions > last_player_cache_evictions {
                tracing::error!(
                    evicted = player_stats.cache.evictions - last_player_cache_evictions,
                    total_evicted = player_stats.cache.evictions,
                    limit = player_stats.cache.capacity,
                    "Public player cache hard limit exceeded"
                );
                last_player_cache_evictions = player_stats.cache.evictions;
            }
            tracing::info!(
                frame_state = ?stats.state,
                active_sessions = stats.sessions.active_sessions,
                write_queue_depth = stats.sessions.outbound_queue_depth,
                rpc_pending = stats.rpc.pending,
                rpc_inbound_active = stats.rpc.inbound_active,
                rpc_pending_rejected = stats.rpc.pending_rejected,
                public_players = player_stats.cache.len,
                public_player_capacity = player_stats.cache.capacity,
                public_player_cache_hits = player_stats.cache.hits,
                public_player_cache_misses = player_stats.cache.misses,
                public_player_cache_evictions = player_stats.cache.evictions,
                dirty_public_players = player_stats.dirty_players,
                public_player_loads = player_stats.load_calls,
                public_player_load_failed = player_stats.load_failed,
                public_player_load_p99_us = player_stats.load_latency.percentile_micros(99.0),
                public_player_saves = player_stats.save_players,
                public_player_save_failed = player_stats.save_failed,
                public_player_save_p99_us = player_stats.save_latency.percentile_micros(99.0),
                public_player_flush_p99_us = player_stats.flush_latency.percentile_micros(99.0),
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
        let config = Config::parse(
            include_str!("../../../config/common.yaml"),
            include_str!("../../../config/public.yaml"),
            include_str!("../../../config/version.json"),
        )
        .unwrap();
        let frame = frame_config(&config).unwrap();

        assert!(frame.service_server.is_some());
        assert!(frame.http.is_none());
        assert_eq!(frame.rpc.pending_capacity(), RPC_PENDING_CAPACITY);
        assert_eq!(frame.node.meta_data().get("protocol").unwrap(), "ss");
    }
}
