use std::{
    collections::HashMap,
    io,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicI32, Ordering},
    },
    time::Duration,
};

use thiserror::Error;
use tokio::task::JoinHandle;
use xframe::{
    Application, ApplicationResult, DiscoveryConfig, FrameConfig, FrameHandle, NodeConfig,
    RpcConfig, ServiceType, ShutdownConfig,
};
use xkk_cache::{delete_service_online, publish_service_online, service_online_ttl};

use crate::{LogicConfig, LogicRuntime, player};
pub use xkk_config::LogicConfig as Config;

#[derive(Debug, Error)]
pub enum ServiceError {
    #[error(transparent)]
    Config(#[from] xkk_config::ConfigError),
    #[error(transparent)]
    Frame(#[from] xframe::Error),
    #[error(transparent)]
    Log(#[from] xlog::Error),
    #[error("close Logic log worker: {0}")]
    LogClose(#[source] io::Error),
    #[error(transparent)]
    Mongo(#[from] xframe::xmongo::Error),
    #[error(transparent)]
    Redis(#[from] xframe::xredis::Error),
    #[error(transparent)]
    Rpc(#[from] xframe::xrpc::Error),
}

fn frame_config(config: &Config) -> Result<FrameConfig, ServiceError> {
    let metadata = HashMap::from([("protocol".to_string(), "ss".to_string())]);
    let node = NodeConfig::new(
        &config.node.cluster,
        ServiceType::Logic,
        config.node.instance_id,
        &config.node.advertise_host,
        config.node.service_port,
    )?
    .with_versions(config.node.pro_version, config.node.conf_version)
    .with_meta_data(metadata);
    let discovery = DiscoveryConfig::new(&config.infrastructure.etcd_dsn)?.with_lease_ttl(
        Duration::from_secs(config.infrastructure.etcd_lease_ttl_seconds),
    )?;
    let transport = xframe::xnet::TransportOptions::default()
        .with_write_queue_capacity(config.capacity.write_queue);
    let listener = xframe::xnet::ServerConfig::with_listener(xframe::xnet::ListenEndpoint::tcp(
        format!("{}:{}", config.node.listen_host, config.node.service_port),
    ))
    .with_transport(transport.clone());

    Ok(FrameConfig::new(node)
        .with_discovery(discovery)
        .with_service_client_transport(transport)
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

fn logic_config(config: &Config) -> LogicConfig {
    LogicConfig {
        resident_capacity: config.capacity.resident_players,
        ttl: Duration::from_secs(config.runtime.player_ttl_seconds),
        shards: config.capacity.mailbox_shards,
        batch_save_count: config.capacity.batch_save_count,
        max_dirty_players: config.capacity.max_dirty_players,
        max_inflight_calls: config.capacity.max_inflight_calls,
        max_inflight_kib: config.capacity.max_inflight_kib,
        max_calls_per_gid: config.capacity.max_calls_per_gid,
        max_kib_per_gid: config.capacity.max_kib_per_gid,
    }
}

pub fn config_path() -> Result<PathBuf, ServiceError> {
    Ok(xkk_config::config_path("logic")?)
}

pub async fn run(config: Config) -> Result<(), ServiceError> {
    config.validate()?;
    let log_options = config.log.options("logs/logic.log");
    let frame_config = frame_config(&config)?;
    let logic_config = logic_config(&config);
    let cluster = config.node.cluster.clone();
    let service_load_interval = Duration::from_secs(config.runtime.service_load_interval_seconds);
    let metrics_interval = Duration::from_secs(config.runtime.metrics_interval_seconds);
    let shutdown_timeout = Duration::from_secs(config.runtime.shutdown_drain_seconds);
    let rpc_timeout = Duration::from_millis(config.runtime.rpc_timeout_ms);
    let mongo_database = config.storage.mongo_database.clone();
    let player_collection = config.storage.player_collection.clone();
    let instance_id = config.node.instance_id;
    let log_guard = xlog::init_global(log_options)?;

    let service: Result<(), ServiceError> = async {
        let prepared = xframe::prepare(frame_config).await?;
        let handle = prepared.handle();
        let mongo = handle
            .mongo()
            .expect("Logic FrameConfig always enables Mongo");
        let redis = handle
            .redis()
            .expect("Logic FrameConfig always enables Redis");
        let application_redis = redis.clone();
        let runtime = LogicRuntime::new(
            logic_config,
            player::persistence(mongo, mongo_database, player_collection),
        );
        let online_count = Arc::new(AtomicI32::new(0));
        player::register_handlers(
            prepared.rpc(),
            handle,
            redis,
            runtime.clone(),
            instance_id,
            online_count.clone(),
            rpc_timeout,
        )?;
        let frame = prepared
            .start(LogicApplication {
                cluster,
                instance_id,
                redis: application_redis,
                service_load_interval,
                metrics_interval,
                shutdown_timeout,
                logic_config,
                runtime,
                online_count,
                service_load_task: None,
                metrics_task: None,
            })
            .await?;
        xlog::info!(instance_id, "Logic service started");
        let shutdown = frame.run_until_shutdown_signal().await;
        xlog::info!(
            instance_id,
            success = shutdown.is_ok(),
            "Logic service stopped"
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

struct LogicApplication {
    cluster: String,
    instance_id: i32,
    redis: xframe::xredis::Client,
    service_load_interval: Duration,
    metrics_interval: Duration,
    shutdown_timeout: Duration,
    logic_config: LogicConfig,
    runtime: LogicRuntime<player::PlayerState, player::PlayerError>,
    online_count: Arc<AtomicI32>,
    service_load_task: Option<JoinHandle<()>>,
    metrics_task: Option<JoinHandle<()>>,
}

impl Application for LogicApplication {
    async fn start(&mut self, frame: FrameHandle) -> ApplicationResult {
        frame.watch(self.cluster.clone(), ServiceType::Gate).await?;
        frame
            .watch_and_connect(self.cluster.clone(), ServiceType::Public)
            .await?;
        let online_count = self.online_count.load(Ordering::Acquire);
        publish_service_online(
            &self.redis,
            &self.cluster,
            ServiceType::Logic,
            self.instance_id,
            online_count,
            service_online_ttl(self.service_load_interval),
        )
        .await?;
        self.service_load_task = Some(spawn_service_online(
            self.redis.clone(),
            self.cluster.clone(),
            self.instance_id,
            self.online_count.clone(),
            self.service_load_interval,
        ));
        xlog::info!(
            resident_players = self.logic_config.resident_capacity,
            max_dirty_players = self.logic_config.max_dirty_players,
            max_inflight_calls = self.logic_config.max_inflight_calls,
            max_calls_per_gid = self.logic_config.max_calls_per_gid,
            "Logic Runtime capacity configured"
        );
        self.metrics_task = spawn_metrics(
            frame,
            self.runtime.clone(),
            self.online_count.clone(),
            self.metrics_interval,
        );
        Ok(())
    }

    async fn shutdown(&mut self, frame: FrameHandle) -> ApplicationResult {
        if let Some(task) = self.service_load_task.take() {
            task.abort();
            let _ = task.await;
        }
        if let Err(error) = delete_service_online(
            &self.redis,
            &self.cluster,
            ServiceType::Logic,
            self.instance_id,
        )
        .await
        {
            xlog::warn!(%error, "Logic service online cleanup failed");
        }
        if let Some(task) = self.metrics_task.take() {
            task.abort();
            let _ = task.await;
        }
        self.runtime
            .shutdown(self.shutdown_timeout)
            .await
            .map_err(|error| Box::new(error) as xframe::ApplicationError)?;
        let logic = self.runtime.stats();
        let rpc = frame.stats().rpc;
        xlog::info!(
            logic_inflight = logic.inflight_calls,
            logic_queued = logic.queued,
            logic_active_gids = logic.active_gids,
            dirty_players = logic.dirty_players,
            rpc_pending = rpc.pending,
            rpc_inbound_active = rpc.inbound_active,
            "Logic Runtime drained"
        );
        Ok(())
    }
}

fn spawn_service_online(
    redis: xframe::xredis::Client,
    cluster: String,
    instance_id: i32,
    online_count: Arc<AtomicI32>,
    interval: Duration,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let ttl = service_online_ttl(interval);
        let mut ticker = tokio::time::interval(interval);
        ticker.tick().await;
        loop {
            ticker.tick().await;
            let online_count = online_count.load(Ordering::Acquire);
            if let Err(error) = publish_service_online(
                &redis,
                &cluster,
                ServiceType::Logic,
                instance_id,
                online_count,
                ttl,
            )
            .await
            {
                xlog::warn!(online_count, %error, "Logic service online publish failed");
            }
        }
    })
}

fn spawn_metrics(
    frame: FrameHandle,
    runtime: LogicRuntime<player::PlayerState, player::PlayerError>,
    online_count: Arc<AtomicI32>,
    interval: Duration,
) -> Option<JoinHandle<()>> {
    if interval.is_zero() {
        return None;
    }
    Some(tokio::spawn(async move {
        let mut ticker = tokio::time::interval(interval);
        ticker.tick().await;
        loop {
            ticker.tick().await;
            let online = online_count.load(Ordering::Acquire);
            let frame_stats = frame.stats();
            let logic_stats = runtime.stats();
            xlog::info!(
                online_players = online,
                frame_state = ?frame_stats.state,
                active_sessions = frame_stats.sessions.active_sessions,
                write_queue_depth = frame_stats.sessions.outbound_queue_depth,
                rpc_pending = frame_stats.rpc.pending,
                rpc_inbound_active = frame_stats.rpc.inbound_active,
                rpc_pending_rejected = frame_stats.rpc.pending_rejected,
                logic_inflight = logic_stats.inflight_calls,
                logic_queued = logic_stats.queued,
                logic_active_gids = logic_stats.active_gids,
                dirty_players = logic_stats.dirty_players,
                dirty_age_ms = logic_stats.oldest_dirty_age.as_millis() as u64,
                logic_rejected = logic_stats.rejected_calls
                    + logic_stats.rejected_kib
                    + logic_stats.rejected_gid
                    + logic_stats.rejected_dirty
                    + logic_stats.rejected_draining,
                logic_p99_us = logic_stats.total_latency.percentile_micros(99.0),
                logic_p999_us = logic_stats.total_latency.percentile_micros(99.9),
                save_failed = logic_stats.save_failed,
                flush_p99_us = logic_stats.flush_latency.percentile_micros(99.0),
                listeners = ?frame_stats.listeners,
                "Logic runtime stats"
            );
        }
    }))
}

#[cfg(test)]
mod config_tests {
    use super::*;

    #[test]
    fn yaml_maps_to_logic_and_frame_capacity() {
        let config = Config::parse(include_str!("../../../config/logic.yaml")).unwrap();
        let logic = logic_config(&config);
        let frame = frame_config(&config).unwrap();

        assert_eq!(logic.max_calls_per_gid, 64);
        assert_eq!(logic.shards, 128);
        assert_eq!(config.storage.mongo_database, "xkk");
        assert_eq!(config.storage.player_collection, "players");
        assert_eq!(config.runtime.rpc_timeout_ms, 3000);
        assert_eq!(config.runtime.service_load_interval_seconds, 3);
        assert_eq!(frame.rpc.pending_capacity(), 100_000);
        assert_eq!(frame.service_client_transport.write_queue_capacity, 1024);
        assert!(frame.service_server.is_some());
        assert_eq!(frame.node.meta_data().get("protocol").unwrap(), "ss");
    }
}
