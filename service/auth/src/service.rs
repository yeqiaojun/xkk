use std::{collections::HashMap, path::PathBuf, time::Duration};

use thiserror::Error;
use tokio::task::JoinHandle;
use xframe::{Application, ApplicationResult, DiscoveryConfig, FrameHandle, FrameState, NodeConfig, RpcConfig, ServiceConfig};
use xkk_cache::load_service_online_counts;
use xkk_protocol::pb;
use xutil::ServiceType;

use crate::api::{AuthApi, LOGIN_PATH, USE_ROLE_PATH};
pub use xkk_config::AuthConfig as Config;

// Auth uses fixed process budgets. If these limits are reached, the owning path rejects work and
// emits an error log; operators scale the role instead of changing per-instance YAML.
const RPC_PENDING_CAPACITY: usize = 1_024;
const HTTP_MAX_BODY_BYTES: usize = 4_096;
const GATE_LOAD_REFRESH_INTERVAL: Duration = Duration::from_secs(3);
const METRICS_REPORT_INTERVAL: Duration = Duration::from_secs(10);

#[derive(Debug, Error)]
pub enum ServiceError {
    #[error(transparent)]
    App(#[from] xkk_app::Error),
    #[error(transparent)]
    Config(#[from] xkk_config::ConfigError),
    #[error(transparent)]
    Frame(#[from] xframe::Error),
    #[error(transparent)]
    Http(#[from] xframe::xhttp::HttpError),
    #[error(transparent)]
    Persist(#[from] xkk_persist::Error),
}

fn service_config(config: &Config) -> Result<ServiceConfig, ServiceError> {
    let metadata = HashMap::from([
        ("protocol".to_string(), "http".to_string()),
        ("login_path".to_string(), LOGIN_PATH.to_string()),
        ("use_role_path".to_string(), USE_ROLE_PATH.to_string()),
    ]);
    let node = NodeConfig::new(
        &config.node.cluster,
        xkk_common::service_type::AUTH,
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
    Ok(xkk_config::config_path("auth")?)
}

pub async fn run(config: Config) -> Result<(), ServiceError> {
    config.validate()?;
    let log_options = config.log.options("logs/auth.log");
    let service_config = service_config(&config)?;
    let cluster = config.node.cluster.clone();
    let instance_id = config.node.instance_id;
    let http_addr = format!("{}:{}", config.node.listen_host, config.node.http_port);
    xkk_app::run(log_options, &config.infrastructure, |resources| async move {
        let xkk_app::Resources { mongo, redis } = resources;
        let mut prepared = xframe::prepare(service_config).await?;
        let frame = prepared.handle();
        let database = xkk_persist::Database::new(mongo.clone())?;
        let application_redis = redis.clone();
        let api = AuthApi::new(frame.clone(), database.accounts(), redis.clone(), &config.security);
        let login = api.clone();
        let use_role = api.clone();
        let ready_handle = frame.clone();
        let http = xframe::xhttp::App::new()
            .with_max_body_bytes(HTTP_MAX_BODY_BYTES)
            .route(LOGIN_PATH, move |ctx, request: pb::AuthLoginReq| {
                let api = login.clone();
                async move { Ok(api.login(ctx, request).await) }
            })?
            .route(USE_ROLE_PATH, move |ctx, request: pb::AuthUseRoleReq| {
                let api = use_role.clone();
                async move { Ok(api.use_role(ctx, request).await) }
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
        let frame = prepared
            .start(AuthApplication::new(cluster, application_redis, GATE_LOAD_REFRESH_INTERVAL, METRICS_REPORT_INTERVAL, api))
            .await?;
        tracing::info!(instance_id, "Auth service started");
        let shutdown = frame.run_until_shutdown_signal().await;
        tracing::info!(instance_id, success = shutdown.is_ok(), "Auth service stopped");
        shutdown?;
        Ok(())
    })
    .await
}

struct AuthApplication {
    cluster: String,
    redis: xredis::Client,
    service_load_interval: Duration,
    metrics_interval: Duration,
    api: AuthApi,
    service_load_task: Option<JoinHandle<()>>,
    metrics_task: Option<JoinHandle<()>>,
}

impl AuthApplication {
    fn new(cluster: String, redis: xredis::Client, service_load_interval: Duration, metrics_interval: Duration, api: AuthApi) -> Self {
        Self { cluster, redis, service_load_interval, metrics_interval, api, service_load_task: None, metrics_task: None }
    }
}

impl Application for AuthApplication {
    async fn start(&mut self, frame: FrameHandle) -> ApplicationResult {
        frame.watch(xkk_common::service_type::GATE).await?;
        refresh_service_online(&frame, &self.redis, &self.cluster, xkk_common::service_type::GATE).await?;
        self.service_load_task =
            Some(spawn_service_loads(frame.clone(), self.redis.clone(), self.cluster.clone(), self.service_load_interval));
        self.metrics_task = spawn_metrics(frame, self.api.clone(), self.metrics_interval);
        Ok(())
    }

    async fn shutdown(&mut self, _frame: FrameHandle) -> ApplicationResult {
        stop_task(&mut self.service_load_task, "Auth service load").await;
        stop_task(&mut self.metrics_task, "Auth metrics").await;
        Ok(())
    }
}

async fn refresh_service_online(
    frame: &FrameHandle,
    redis: &xredis::Client,
    cluster: &str,
    service_type: ServiceType,
) -> ApplicationResult {
    let instance_ids = frame.service_instance_ids(service_type)?;
    let counts = load_service_online_counts(redis, cluster, service_type.as_i32(), instance_ids).await?;
    frame.update_online_counts(service_type, counts.into_iter().map(|count| (count.instance_id, count.online_count)))?;
    Ok(())
}

fn spawn_service_loads(frame: FrameHandle, redis: xredis::Client, cluster: String, interval: Duration) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(interval);
        ticker.tick().await;
        loop {
            ticker.tick().await;
            if let Err(error) = refresh_service_online(&frame, &redis, &cluster, xkk_common::service_type::GATE).await {
                tracing::warn!(%error, "Auth Gate online refresh failed");
            }
        }
    })
}

fn spawn_metrics(frame: FrameHandle, api: AuthApi, interval: Duration) -> Option<JoinHandle<()>> {
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
                    "Auth RPC pending hard limit exceeded"
                );
                last_rpc_pending_rejected = stats.rpc.pending_rejected;
            }
            tracing::info!(
                frame_state = ?stats.state,
                available_request_slots = api.available_request_slots(),
                rpc_pending = stats.rpc.pending,
                rpc_pending_rejected = stats.rpc.pending_rejected,
                "Auth runtime stats"
            );
        }
    }))
}

async fn stop_task(task: &mut Option<JoinHandle<()>>, name: &'static str) {
    let Some(task) = task.take() else {
        return;
    };
    task.abort();
    if let Err(error) = task.await
        && !error.is_cancelled()
    {
        tracing::error!(task = name, %error, "Auth background task failed");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn example_config_builds_auth_service_runtime() {
        let config = Config::parse(
            include_str!("../../../config/common.yaml"),
            include_str!("../../../config/auth.yaml"),
            include_str!("../../../config/version.json"),
        )
        .unwrap();
        let frame = service_config(&config).unwrap();

        assert!(frame.service_server.is_none());
        assert_eq!(frame.rpc, RpcConfig::default().with_pending_capacity(RPC_PENDING_CAPACITY));
        assert_eq!(frame.node.meta_data().get("login_path").unwrap(), LOGIN_PATH);
    }
}
