use std::{collections::HashMap, io, path::PathBuf, time::Duration};

use thiserror::Error;
use tokio::task::JoinHandle;
use xframe::{
    Application, ApplicationResult, DiscoveryConfig, FrameConfig, FrameHandle, FrameState,
    HttpServerConfig, NodeConfig, RpcConfig, ServiceType, ShutdownConfig,
};
use xkk_protocol::pb;

use crate::query::QueryApi;

pub use xkk_config::QueryConfig as Config;

// These are stable HTTP/RPC process conventions. They intentionally do not
// vary by deployment; overload is rejected and reported at error level.
const RPC_PENDING_CAPACITY: usize = 100_000;
const HTTP_MAX_BODY_BYTES: usize = 8_192;
const SHUTDOWN_DRAIN_TIMEOUT: Duration = Duration::from_secs(10);
const METRICS_REPORT_INTERVAL: Duration = Duration::from_secs(10);

#[derive(Debug, Error)]
pub enum ServiceError {
    #[error(transparent)]
    Config(#[from] xkk_config::ConfigError),
    #[error(transparent)]
    Frame(#[from] xframe::Error),
    #[error(transparent)]
    Http(#[from] xframe::xhttp::HttpError),
    #[error(transparent)]
    Log(#[from] xlog::Error),
    #[error("close Query log worker: {0}")]
    LogClose(#[source] io::Error),
    #[error(transparent)]
    Mongo(#[from] xframe::xmongo::Error),
    #[error(transparent)]
    Persist(#[from] xkk_persist::Error),
    #[error(transparent)]
    Redis(#[from] xframe::xredis::Error),
    #[error(transparent)]
    Protocol(#[from] xkk_protocol::ProtocolError),
}

fn frame_config(config: &Config) -> Result<FrameConfig, ServiceError> {
    let metadata = HashMap::from([
        ("protocol".to_string(), "http".to_string()),
        ("health_path".to_string(), "/healthz".to_string()),
        ("ready_path".to_string(), "/readyz".to_string()),
    ]);
    let node = NodeConfig::new(
        &config.node.cluster,
        ServiceType::Query,
        config.node.instance_id,
        &config.node.advertise_host,
        config.node.http_port,
    )?
    .with_versions(config.version.program, config.version.conf)
    .with_meta_data(metadata);
    let discovery = DiscoveryConfig::new(&config.infrastructure.etcd_dsn)?;

    Ok(FrameConfig::new(node)
        .with_discovery(discovery)
        .with_mongo(xframe::xmongo::Config::new(
            &config.infrastructure.mongo_dsn,
        )?)
        .with_redis(xframe::xredis::RedisConfig::new(
            &config.infrastructure.redis_dsn,
        )?)
        .with_http(HttpServerConfig::new(format!(
            "{}:{}",
            config.node.listen_host, config.node.http_port
        ))?)
        .with_rpc(RpcConfig::new(RPC_PENDING_CAPACITY)?)
        .with_shutdown(ShutdownConfig::new(SHUTDOWN_DRAIN_TIMEOUT)?))
}

pub fn config_path() -> Result<PathBuf, ServiceError> {
    Ok(xkk_config::config_path("query")?)
}

pub async fn run(config: Config) -> Result<(), ServiceError> {
    config.validate()?;
    let log_options = config.log.options("logs/query.log");
    let frame_config = frame_config(&config)?;
    let instance_id = config.node.instance_id;
    let log_guard = xlog::init_global(log_options)?;

    let service: Result<(), ServiceError> = async {
        xkk_protocol::init_global_registry()?;
        let mut prepared = xframe::prepare(frame_config).await?;
        let handle = prepared.handle();
        let mongo = handle
            .mongo()
            .expect("Query FrameConfig always enables Mongo");
        let collections = xkk_persist::Collections::new(mongo)?;
        let api = QueryApi::new(collections.players());
        let ready_handle = handle.clone();
        let gamer_info = api.clone();
        let http = xframe::xhttp::App::new()
            .with_max_body_bytes(HTTP_MAX_BODY_BYTES)
            .route(
                "/v1/query/gamers",
                move |_ctx, request: pb::GamerInfoReq| {
                    let api = gamer_info.clone();
                    async move { Ok(api.gamer_info(request).await) }
                },
            )?
            .get("/healthz", |_| async { xframe::xhttp::StatusCode::OK })?
            .get("/readyz", move |_| {
                let frame = ready_handle.clone();
                async move {
                    if frame.state() == FrameState::Running {
                        xframe::xhttp::StatusCode::OK
                    } else {
                        xframe::xhttp::StatusCode::SERVICE_UNAVAILABLE
                    }
                }
            })?;
        prepared.set_http_app(http)?;
        let frame = prepared
            .start(QueryApplication::new(METRICS_REPORT_INTERVAL, api))
            .await?;
        tracing::info!(instance_id, "Query service started");
        let shutdown = frame.run_until_shutdown_signal().await;
        tracing::info!(
            instance_id,
            success = shutdown.is_ok(),
            "Query service stopped"
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

struct QueryApplication {
    metrics_interval: Duration,
    api: QueryApi,
    metrics_task: Option<JoinHandle<()>>,
}

impl QueryApplication {
    fn new(metrics_interval: Duration, api: QueryApi) -> Self {
        Self {
            metrics_interval,
            api,
            metrics_task: None,
        }
    }
}

impl Application for QueryApplication {
    async fn start(&mut self, frame: FrameHandle) -> ApplicationResult {
        self.metrics_task = spawn_metrics(frame, self.api.clone(), self.metrics_interval);
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

fn spawn_metrics(frame: FrameHandle, api: QueryApi, interval: Duration) -> Option<JoinHandle<()>> {
    if interval.is_zero() {
        return None;
    }
    Some(tokio::spawn(async move {
        let mut ticker = tokio::time::interval(interval);
        let mut last_rpc_pending_rejected = 0;
        ticker.tick().await;
        loop {
            ticker.tick().await;
            let stats = frame.stats();
            if stats.rpc.pending_rejected > last_rpc_pending_rejected {
                tracing::error!(
                    rejected = stats.rpc.pending_rejected - last_rpc_pending_rejected,
                    total_rejected = stats.rpc.pending_rejected,
                    limit = RPC_PENDING_CAPACITY,
                    "Query RPC pending hard limit exceeded"
                );
                last_rpc_pending_rejected = stats.rpc.pending_rejected;
            }
            tracing::info!(
                frame_state = ?stats.state,
                available_request_slots = api.available_request_slots(),
                active_sessions = stats.sessions.active_sessions,
                rpc_pending = stats.rpc.pending,
                rpc_inbound_active = stats.rpc.inbound_active,
                rpc_pending_rejected = stats.rpc.pending_rejected,
                "Query runtime stats"
            );
        }
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn example_config_builds_http_only_frame() {
        let config = Config::parse(
            include_str!("../../../config/common.yaml"),
            include_str!("../../../config/query.yaml"),
            include_str!("../../../config/version.json"),
        )
        .unwrap();
        let frame = frame_config(&config).unwrap();

        assert!(frame.http.is_some());
        assert!(frame.service_server.is_none());
        assert_eq!(frame.rpc.pending_capacity(), RPC_PENDING_CAPACITY);
        assert_eq!(frame.node.meta_data().get("protocol").unwrap(), "http");
    }
}
