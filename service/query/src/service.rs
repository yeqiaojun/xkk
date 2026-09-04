use std::{collections::HashMap, io, path::PathBuf, time::Duration};

use thiserror::Error;
use tokio::task::JoinHandle;
use xframe::{Application, ApplicationResult, DiscoveryConfig, FrameHandle, FrameState, NodeConfig, RpcConfig, ServiceConfig};
use xkk_protocol::pb;

use crate::query::QueryApi;

pub use xkk_config::QueryConfig as Config;

// These are stable HTTP/RPC process conventions. They intentionally do not
// vary by deployment; overload is rejected and reported at error level.
const RPC_PENDING_CAPACITY: usize = 100_000;
const HTTP_MAX_BODY_BYTES: usize = 8_192;
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
    Mongo(#[from] xmongo::Error),
    #[error(transparent)]
    Persist(#[from] xkk_persist::Error),
    #[error(transparent)]
    Redis(#[from] xredis::Error),
    #[error(transparent)]
    Protocol(#[from] xkk_protocol::ProtocolError),
}

fn service_config(config: &Config) -> Result<ServiceConfig, ServiceError> {
    let metadata = HashMap::from([
        ("protocol".to_string(), "http".to_string()),
        ("health_path".to_string(), "/healthz".to_string()),
        ("ready_path".to_string(), "/readyz".to_string()),
    ]);
    let node = NodeConfig::new(
        &config.node.cluster,
        xkk_common::service_type::QUERY,
        config.node.instance_id,
        &config.node.advertise_host,
        config.node.http_port,
    )?
    .with_versions(config.version.program, config.version.conf)
    .with_meta_data(metadata);
    let discovery = DiscoveryConfig::new(&config.infrastructure.etcd_dsn)?;

    Ok(ServiceConfig::new(node).with_discovery(discovery).with_rpc(RpcConfig::default().with_pending_capacity(RPC_PENDING_CAPACITY)))
}

pub fn config_path() -> Result<PathBuf, ServiceError> {
    Ok(xkk_config::config_path("query")?)
}

pub async fn run(config: Config) -> Result<(), ServiceError> {
    config.validate()?;
    let log_options = config.log.options("logs/query.log");
    let service_config = service_config(&config)?;
    let instance_id = config.node.instance_id;
    let http_addr = format!("{}:{}", config.node.listen_host, config.node.http_port);
    let log_guard = xlog::init_global(log_options)?;

    let service: Result<(), ServiceError> = async {
        xkk_common::service_type::init();
        xkk_protocol::init_global_registry()?;
        let redis = xredis::Client::connect_config(xredis::RedisConfig::new(&config.infrastructure.redis_dsn)?).await?;
        let mongo = match xmongo::Client::connect_config(xmongo::Config::new(&config.infrastructure.mongo_dsn)?).await {
            Ok(mongo) => mongo,
            Err(error) => {
                redis.close();
                return Err(error.into());
            }
        };
        let result: Result<(), ServiceError> = async {
            let mut prepared = xframe::prepare(service_config).await?;
            let handle = prepared.handle();
            let database = xkk_persist::Database::new(mongo.clone())?;
            let api = QueryApi::new(database.players());
            let ready_handle = handle.clone();
            let gamer_info = api.clone();
            let http = xframe::xhttp::App::new()
                .with_max_body_bytes(HTTP_MAX_BODY_BYTES)
                .route("/v1/query/gamers", move |_ctx, request: pb::GamerInfoReq| {
                    let api = gamer_info.clone();
                    async move { Ok(api.gamer_info(request).await) }
                })?
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
            prepared.set_http_app(http_addr, http)?;
            let frame = prepared.start(QueryApplication::new(METRICS_REPORT_INTERVAL, api)).await?;
            tracing::info!(instance_id, "Query service started");
            let shutdown = frame.run_until_shutdown_signal().await;
            tracing::info!(instance_id, success = shutdown.is_ok(), "Query service stopped");
            shutdown?;
            Ok(())
        }
        .await;
        mongo.shutdown().await;
        redis.close();
        result
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
        Self { metrics_interval, api, metrics_task: None }
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
    fn example_config_builds_query_service_runtime() {
        let config = Config::parse(
            include_str!("../../../config/common.yaml"),
            include_str!("../../../config/query.yaml"),
            include_str!("../../../config/version.json"),
        )
        .unwrap();
        let frame = service_config(&config).unwrap();

        assert!(frame.service_server.is_none());
        assert_eq!(frame.rpc, RpcConfig::default().with_pending_capacity(RPC_PENDING_CAPACITY));
        assert_eq!(frame.node.meta_data().get("protocol").unwrap(), "http");
    }
}
