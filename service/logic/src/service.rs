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

use crate::{LogicConfig, LogicRuntime, player, stats::LoginMetrics};
pub use xkk_config::LogicConfig as Config;

// These limits are product/runtime invariants. Keep them next to the Logic
// composition that consumes them; changing one requires code review and a build.
const SERVICE_WRITE_QUEUE_CAPACITY: usize = 1_024;
const RPC_PENDING_CAPACITY: usize = 100_000;
const RPC_CALL_TIMEOUT: Duration = Duration::from_secs(3);
const SHUTDOWN_DRAIN_TIMEOUT: Duration = Duration::from_secs(10);
const SERVICE_LOAD_PUBLISH_INTERVAL: Duration = Duration::from_secs(3);
const METRICS_REPORT_INTERVAL: Duration = Duration::from_secs(10);

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
        ServiceType::Logic,
        config.node.instance_id,
        &config.node.advertise_host,
        config.node.service_port,
    )?
    .with_versions(config.version.program, config.version.conf)
    .with_meta_data(metadata);
    let discovery = DiscoveryConfig::new(&config.infrastructure.etcd_dsn)?;
    let transport = xframe::xnet::TransportOptions::default()
        .with_write_queue_capacity(SERVICE_WRITE_QUEUE_CAPACITY);
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
        .with_rpc(RpcConfig::new(RPC_PENDING_CAPACITY)?)
        .with_shutdown(ShutdownConfig::new(SHUTDOWN_DRAIN_TIMEOUT)?))
}

fn logic_config() -> LogicConfig {
    LogicConfig::HARD_LIMITS
}

pub fn config_path() -> Result<PathBuf, ServiceError> {
    Ok(xkk_config::config_path("logic")?)
}

pub async fn run(config: Config) -> Result<(), ServiceError> {
    config.validate()?;
    let log_options = config.log.options("logs/logic.log");
    let frame_config = frame_config(&config)?;
    let logic_config = logic_config();
    let cluster = config.node.cluster.clone();
    let instance_id = config.node.instance_id;
    let log_guard = xlog::init_global(log_options)?;

    let service: Result<(), ServiceError> = async {
        xkk_protocol::init_global_registry()?;
        let prepared = xframe::prepare(frame_config).await?;
        let handle = prepared.handle();
        let mongo = handle
            .mongo()
            .expect("Logic FrameConfig always enables Mongo");
        let redis = handle
            .redis()
            .expect("Logic FrameConfig always enables Redis");
        let collections = xkk_persist::Collections::new(mongo)?;
        let application_redis = redis.clone();
        let login_metrics = Arc::new(LoginMetrics::default());
        let runtime = LogicRuntime::new(
            logic_config,
            player::persistence(collections.players(), login_metrics.clone()),
        );
        let online_count = Arc::new(AtomicI32::new(0));
        player::register_handlers(
            prepared.rpc(),
            handle,
            redis,
            runtime.clone(),
            instance_id,
            online_count.clone(),
            RPC_CALL_TIMEOUT,
            login_metrics.clone(),
        )?;
        let frame = prepared
            .start(LogicApplication {
                cluster,
                instance_id,
                redis: application_redis,
                service_load_interval: SERVICE_LOAD_PUBLISH_INTERVAL,
                metrics_interval: METRICS_REPORT_INTERVAL,
                shutdown_timeout: SHUTDOWN_DRAIN_TIMEOUT,
                logic_config,
                runtime,
                online_count,
                login_metrics,
                service_load_task: None,
                metrics_task: None,
            })
            .await?;
        tracing::info!(instance_id, "Logic service started");
        let shutdown = frame.run_until_shutdown_signal().await;
        tracing::info!(
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
    login_metrics: Arc<LoginMetrics>,
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
            ServiceType::Logic.as_i32(),
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
        tracing::info!(
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
            self.login_metrics.clone(),
            self.metrics_interval,
        );
        Ok(())
    }

    async fn shutdown(&mut self, frame: FrameHandle) -> ApplicationResult {
        stop_task(&mut self.service_load_task, "Logic service load").await;
        if let Err(error) = delete_service_online(
            &self.redis,
            &self.cluster,
            ServiceType::Logic.as_i32(),
            self.instance_id,
        )
        .await
        {
            tracing::warn!(%error, "Logic service online cleanup failed");
        }
        stop_task(&mut self.metrics_task, "Logic metrics").await;
        self.runtime
            .shutdown(self.shutdown_timeout)
            .await
            .map_err(|error| Box::new(error) as xframe::ApplicationError)?;
        let logic = self.runtime.stats();
        let rpc = frame.stats().rpc;
        tracing::info!(
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
                ServiceType::Logic.as_i32(),
                instance_id,
                online_count,
                ttl,
            )
            .await
            {
                tracing::warn!(online_count, %error, "Logic service online publish failed");
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
        tracing::error!(task = name, %error, "Logic background task failed");
    }
}

fn spawn_metrics(
    frame: FrameHandle,
    runtime: LogicRuntime<player::PlayerState, player::PlayerError>,
    online_count: Arc<AtomicI32>,
    login_metrics: Arc<LoginMetrics>,
    interval: Duration,
) -> Option<JoinHandle<()>> {
    if interval.is_zero() {
        return None;
    }
    Some(tokio::spawn(async move {
        let mut ticker = tokio::time::interval(interval);
        let mut last_rpc_pending_rejected = 0;
        let mut last_write_queue_rejected = 0;
        let mut last_rejected_calls = 0;
        let mut last_rejected_kib = 0;
        let mut last_rejected_gid = 0;
        let mut last_rejected_dirty = 0;
        ticker.tick().await;
        loop {
            ticker.tick().await;
            let online = online_count.load(Ordering::Acquire);
            let frame_stats = frame.stats();
            let logic_stats = runtime.stats();
            let login = login_metrics.snapshot();
            let write_queue_rejected = frame_stats.sessions.outbound_rejected_full
                + frame_stats
                    .listeners
                    .iter()
                    .map(|listener| listener.outbound_rejected_full)
                    .sum::<u64>();
            if frame_stats.rpc.pending_rejected > last_rpc_pending_rejected {
                tracing::error!(
                    rejected = frame_stats.rpc.pending_rejected - last_rpc_pending_rejected,
                    total_rejected = frame_stats.rpc.pending_rejected,
                    limit = RPC_PENDING_CAPACITY,
                    "Logic RPC pending hard limit exceeded"
                );
                last_rpc_pending_rejected = frame_stats.rpc.pending_rejected;
            }
            if write_queue_rejected > last_write_queue_rejected {
                tracing::error!(
                    rejected = write_queue_rejected - last_write_queue_rejected,
                    total_rejected = write_queue_rejected,
                    limit = SERVICE_WRITE_QUEUE_CAPACITY,
                    "Logic write queue hard limit exceeded"
                );
                last_write_queue_rejected = write_queue_rejected;
            }
            if logic_stats.rejected_calls > last_rejected_calls {
                tracing::error!(
                    rejected = logic_stats.rejected_calls - last_rejected_calls,
                    total_rejected = logic_stats.rejected_calls,
                    limit = LogicConfig::HARD_LIMITS.max_inflight_calls,
                    "Logic inflight call hard limit exceeded"
                );
                last_rejected_calls = logic_stats.rejected_calls;
            }
            if logic_stats.rejected_kib > last_rejected_kib {
                tracing::error!(
                    rejected = logic_stats.rejected_kib - last_rejected_kib,
                    total_rejected = logic_stats.rejected_kib,
                    limit_kib = LogicConfig::HARD_LIMITS.max_inflight_kib,
                    "Logic inflight payload hard limit exceeded"
                );
                last_rejected_kib = logic_stats.rejected_kib;
            }
            if logic_stats.rejected_gid > last_rejected_gid {
                tracing::error!(
                    rejected = logic_stats.rejected_gid - last_rejected_gid,
                    total_rejected = logic_stats.rejected_gid,
                    call_limit = LogicConfig::HARD_LIMITS.max_calls_per_gid,
                    kib_limit = LogicConfig::HARD_LIMITS.max_kib_per_gid,
                    "Logic per-player mailbox hard limit exceeded"
                );
                last_rejected_gid = logic_stats.rejected_gid;
            }
            if logic_stats.rejected_dirty > last_rejected_dirty {
                tracing::error!(
                    rejected = logic_stats.rejected_dirty - last_rejected_dirty,
                    total_rejected = logic_stats.rejected_dirty,
                    limit = LogicConfig::HARD_LIMITS.max_dirty_players,
                    "Logic dirty player hard limit exceeded"
                );
                last_rejected_dirty = logic_stats.rejected_dirty;
            }
            tracing::info!(
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
                logic_queue_p99_us = logic_stats.queue_latency.percentile_micros(99.0),
                logic_load_p99_us = logic_stats.load_latency.percentile_micros(99.0),
                logic_preload_p99_us = logic_stats.preload_latency.percentile_micros(99.0),
                logic_run_p99_us = logic_stats.run_latency.percentile_micros(99.0),
                logic_p99_us = logic_stats.total_latency.percentile_micros(99.0),
                logic_p999_us = logic_stats.total_latency.percentile_micros(99.9),
                login_count = login.total.count(),
                login_avg_us = login.total.average_micros(),
                login_p99_us = login.total.percentile_micros(99.0),
                login_max_us = login.total.max_micros,
                login_runtime_avg_us = login.runtime_wait.average_micros(),
                login_runtime_p99_us = login.runtime_wait.percentile_micros(99.0),
                login_mongo_find_avg_us = login.mongo_find.average_micros(),
                login_mongo_find_p99_us = login.mongo_find.percentile_micros(99.0),
                login_mongo_create_avg_us = login.mongo_create.average_micros(),
                login_mongo_create_p99_us = login.mongo_create.percentile_micros(99.0),
                login_redis_owner_avg_us = login.redis_owner.average_micros(),
                login_redis_owner_p99_us = login.redis_owner.percentile_micros(99.0),
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
        let config = Config::parse(
            include_str!("../../../config/common.yaml"),
            include_str!("../../../config/logic.yaml"),
            include_str!("../../../config/version.json"),
        )
        .unwrap();
        let logic = logic_config();
        let frame = frame_config(&config).unwrap();

        assert_eq!(logic.max_calls_per_gid, 64);
        assert_eq!(logic.shards, 128);
        assert_eq!(config.version.conf, 0);
        assert_eq!(frame.rpc.pending_capacity(), RPC_PENDING_CAPACITY);
        assert_eq!(
            frame.service_client_transport.write_queue_capacity,
            SERVICE_WRITE_QUEUE_CAPACITY
        );
        assert!(frame.service_server.is_some());
        assert_eq!(frame.node.meta_data().get("protocol").unwrap(), "ss");
    }
}
