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
    Redis(#[from] xframe::xredis::Error),
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
    .with_versions(config.node.pro_version, config.node.conf_version)
    .with_meta_data(metadata);
    let discovery = DiscoveryConfig::new(&config.infrastructure.etcd_dsn)?.with_lease_ttl(
        Duration::from_secs(config.infrastructure.etcd_lease_ttl_seconds),
    )?;

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
        .with_rpc(RpcConfig::new(config.capacity.rpc_pending)?)
        .with_shutdown(ShutdownConfig::new(Duration::from_secs(
            config.runtime.shutdown_drain_seconds,
        ))?))
}

pub fn config_path() -> Result<PathBuf, ServiceError> {
    Ok(xkk_config::config_path("query")?)
}

pub async fn run(config: Config) -> Result<(), ServiceError> {
    config.validate()?;
    let log_options = config.log.options("logs/query.log");
    let frame_config = frame_config(&config)?;
    let metrics_interval = Duration::from_secs(config.runtime.metrics_interval_seconds);
    let instance_id = config.node.instance_id;
    let max_body_bytes = config.capacity.max_http_body_bytes;
    let log_guard = xlog::init_global(log_options)?;

    let service: Result<(), ServiceError> = async {
        let mut prepared = xframe::prepare(frame_config).await?;
        let handle = prepared.handle();
        let mongo = handle
            .mongo()
            .expect("Query FrameConfig always enables Mongo");
        let api = QueryApi::new(
            mongo,
            &config.storage.mongo_database,
            &config.storage.player_collection,
            &config.storage.manifest_collection,
            pb::ConfigManifestData {
                version: config.storage.current_manifest_version.clone(),
                key: config.storage.current_manifest_key.clone(),
                base_url: config.storage.current_manifest_base_url.clone(),
                files: Vec::new(),
            },
            config.capacity.max_gamer_ids,
            config.capacity.max_inflight_requests,
        );
        api.initialize().await?;
        let ready_handle = handle.clone();
        let gamer_info = api.clone();
        let config_key = api.clone();
        let config_manifest = api.clone();
        let http = xframe::xhttp::App::new()
            .with_max_body_bytes(max_body_bytes)
            .route(
                "/v1/query/gamers",
                move |_ctx, request: pb::GamerInfoReq| {
                    let api = gamer_info.clone();
                    async move { Ok(api.gamer_info(request).await) }
                },
            )?
            .route(
                "/v1/query/config/key",
                move |_ctx, request: pb::ConfigKeyReq| {
                    let api = config_key.clone();
                    async move { Ok(api.config_key(request).await) }
                },
            )?
            .route(
                "/v1/query/config/manifest",
                move |_ctx, request: pb::ConfigManifestReq| {
                    let api = config_manifest.clone();
                    async move { Ok(api.config_manifest(request).await) }
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
            .start(QueryApplication::new(metrics_interval, api))
            .await?;
        xlog::info!(instance_id, "Query service started");
        let shutdown = frame.run_until_shutdown_signal().await;
        xlog::info!(
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
        ticker.tick().await;
        loop {
            ticker.tick().await;
            let stats = frame.stats();
            xlog::info!(
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
        let config = Config::parse(include_str!("../../../config/query.yaml")).unwrap();
        let frame = frame_config(&config).unwrap();

        assert!(frame.http.is_some());
        assert!(frame.service_server.is_none());
        assert_eq!(config.capacity.max_gamer_ids, 100);
        assert_eq!(frame.node.meta_data().get("protocol").unwrap(), "http");
    }
}
