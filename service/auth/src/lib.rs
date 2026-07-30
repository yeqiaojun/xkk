use std::{collections::HashMap, io, path::PathBuf, sync::Arc, time::Duration};

use thiserror::Error;
use tokio::{sync::Semaphore, task::JoinHandle};
use xframe::{
    Application, ApplicationResult, DiscoveryConfig, FrameConfig, FrameHandle, FrameState,
    HttpServerConfig, NodeConfig, RpcConfig, ServiceType, ShutdownConfig,
    xmongo::{self, mongodb::bson::Document},
    xservice::ServiceStatus,
};
use xkk_cache::{
    allocate_gid, enqueue_login, leave_login_queue, load_online, refresh_service_online, set_token,
};
use xkk_common::{credential_hash, unix_millis, unix_seconds};
pub use xkk_config::AuthConfig as Config;
use xkk_persist::{load_model, save_model};
use xkk_protocol::{code, error_status, ok_status, pb};
use xtoken::TokenCoder;

const LOGIN_PATH: &str = "/v1/auth/login";
const USE_ROLE_PATH: &str = "/v1/auth/use-role";

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
    #[error("close Auth log worker: {0}")]
    LogClose(#[source] io::Error),
    #[error(transparent)]
    Mongo(#[from] xmongo::Error),
    #[error(transparent)]
    Redis(#[from] xframe::xredis::Error),
}

fn frame_config(config: &Config) -> Result<FrameConfig, ServiceError> {
    let metadata = HashMap::from([
        ("protocol".to_string(), "http".to_string()),
        ("login_path".to_string(), LOGIN_PATH.to_string()),
        ("use_role_path".to_string(), USE_ROLE_PATH.to_string()),
    ]);
    let node = NodeConfig::new(
        &config.node.cluster,
        ServiceType::Auth,
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
        .with_mongo(xmongo::Config::new(&config.infrastructure.mongo_dsn)?)
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
    Ok(xkk_config::config_path("auth")?)
}

pub async fn run(config: Config) -> Result<(), ServiceError> {
    config.validate()?;
    let log_options = config.log.options("logs/auth.log");
    let frame_config = frame_config(&config)?;
    let cluster = config.node.cluster.clone();
    let service_load_interval = Duration::from_secs(config.runtime.service_load_interval_seconds);
    let metrics_interval = Duration::from_secs(config.runtime.metrics_interval_seconds);
    let instance_id = config.node.instance_id;
    let max_body_bytes = config.capacity.max_http_body_bytes;
    let log_guard = xlog::init_global(log_options)?;

    let service: Result<(), ServiceError> = async {
        let mut prepared = xframe::prepare(frame_config).await?;
        let frame = prepared.handle();
        let mongo = frame
            .mongo()
            .expect("Auth FrameConfig always enables Mongo");
        let redis = frame
            .redis()
            .expect("Auth FrameConfig always enables Redis");
        let application_redis = redis.clone();
        let api = AuthApi::new(frame.clone(), mongo, redis, &config);
        let login = api.clone();
        let use_role = api.clone();
        let ready_handle = frame.clone();
        let http = xframe::xhttp::App::new()
            .with_max_body_bytes(max_body_bytes)
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
        prepared.set_http_app(http)?;
        let frame = prepared
            .start(AuthApplication::new(
                cluster,
                application_redis,
                service_load_interval,
                metrics_interval,
                api,
            ))
            .await?;
        xlog::info!(instance_id, "Auth service started");
        let shutdown = frame.run_until_shutdown_signal().await;
        xlog::info!(
            instance_id,
            success = shutdown.is_ok(),
            "Auth service stopped"
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

#[derive(Clone)]
struct AuthApi {
    frame: FrameHandle,
    redis: xframe::xredis::Client,
    accounts: xmongo::Collection<Document>,
    token: TokenCoder,
    login_global: xframe::xredis::RateLimiter,
    login_per_ip: xframe::xredis::RateLimiter,
    role_admission: xframe::xredis::RateLimiter,
    inflight: Arc<Semaphore>,
    gate_player_capacity: i32,
    login_queue_capacity: i64,
    login_queue_retry_seconds: i64,
    login_queue_entry_ttl: Duration,
    account_lock_ttl: Duration,
}

impl AuthApi {
    fn new(
        frame: FrameHandle,
        mongo: xmongo::Client,
        redis: xframe::xredis::Client,
        config: &Config,
    ) -> Self {
        Self {
            accounts: mongo.collection(
                &config.storage.mongo_database,
                &config.storage.account_collection,
            ),
            token: TokenCoder::new(
                &config.security.token_secret,
                config.security.token_expire_seconds,
            ),
            login_global: redis.rate_limiter(
                "xkk:auth:login:global",
                config.capacity.login_global_limit,
                Duration::from_millis(config.runtime.login_rate_window_ms),
            ),
            login_per_ip: redis.rate_limiter(
                "xkk:auth:login:ip",
                config.capacity.login_per_ip_limit,
                Duration::from_millis(config.runtime.login_rate_window_ms),
            ),
            role_admission: redis.rate_limiter(
                "xkk:auth:role:admission",
                config.capacity.role_admission_limit,
                Duration::from_millis(config.runtime.role_admission_window_ms),
            ),
            inflight: Arc::new(Semaphore::new(config.capacity.max_inflight_requests)),
            gate_player_capacity: config.capacity.gate_player_capacity,
            login_queue_capacity: config.capacity.login_queue_capacity,
            login_queue_retry_seconds: config.runtime.login_queue_retry_seconds,
            login_queue_entry_ttl: Duration::from_secs(
                config.runtime.login_queue_entry_ttl_seconds,
            ),
            account_lock_ttl: Duration::from_secs(config.runtime.account_lock_seconds),
            frame,
            redis,
        }
    }

    async fn login(
        &self,
        context: xframe::xhttp::RequestContext,
        request: pb::AuthLoginReq,
    ) -> pb::AuthLoginRsp {
        let Ok(_permit) = self.inflight.clone().try_acquire_owned() else {
            return login_error(code::OVERLOADED, "Auth request capacity exhausted");
        };
        let Some(device) = request.device.as_ref() else {
            return login_error(code::INVALID_ARGUMENT, "device is required");
        };
        if request.account.is_empty()
            || request.account.len() > 64
            || request.credential.is_empty()
            || request.credential.len() > 256
            || device.device_id.len() < 8
        {
            return login_error(code::INVALID_ARGUMENT, "invalid Auth login request");
        }

        let ip = context
            .client_ip()
            .map(|ip| ip.to_string())
            .unwrap_or_else(|| "unknown".to_string());
        let global = self.login_global.allow("all").await;
        let per_ip = self.login_per_ip.allow(&ip).await;
        match (global, per_ip) {
            (Ok(global), Ok(per_ip)) if global.allowed && per_ip.allowed => {}
            (Ok(_), Ok(_)) => return login_error(code::RATE_LIMITED, "login rate exceeded"),
            (Err(error), _) | (_, Err(error)) => {
                xlog::error!(account = %request.account, %error, "Auth login limiter failed");
                return login_error(code::INTERNAL, "login limiter failed");
            }
        }

        let account = match self
            .load_or_create_account(&request.account, &request.credential)
            .await
        {
            Ok(account) => account,
            Err(status) => {
                return pb::AuthLoginRsp {
                    status: Some(status),
                    ..Default::default()
                };
            }
        };
        let Some(role) = account.roles.first() else {
            return login_error(code::CONFLICT, "account has no role");
        };
        let token = match self.token.simple_token_encode(role.gid, &device.device_id) {
            Ok(token) => token,
            Err(error) => {
                xlog::error!(gid = role.gid, %error, "Auth token encode failed");
                return login_error(code::INTERNAL, "token encode failed");
            }
        };
        if let Err(error) = set_token(&self.redis, role.gid, &account.account, &token).await {
            xlog::error!(gid = role.gid, %error, "Auth Redis token save failed");
            return login_error(code::INTERNAL, "token save failed");
        }

        xlog::info!(
            account = %account.account,
            gid = role.gid,
            client_ip = %ip,
            roles = account.roles.len(),
            "Auth login succeeded"
        );
        pb::AuthLoginRsp {
            status: Some(ok_status()),
            account: account.account,
            token,
            roles: account.roles,
            created_at: account.created_at,
        }
    }

    async fn load_or_create_account(
        &self,
        account: &str,
        credential: &str,
    ) -> Result<pb::AccountData, pb::Status> {
        let lock_key = format!("xkk:account:lock:{account}");
        let lock = match self.redis.try_lock(lock_key, self.account_lock_ttl).await {
            Ok(Some(lock)) => lock,
            Ok(None) => {
                return Err(error_status(
                    code::RATE_LIMITED,
                    "account login in progress",
                ));
            }
            Err(error) => {
                xlog::error!(account, %error, "Auth account lock failed");
                return Err(error_status(code::INTERNAL, "account lock failed"));
            }
        };

        let result = self
            .load_or_create_account_locked(account, credential)
            .await;
        if let Err(error) = lock.release().await {
            xlog::warn!(account, %error, "Auth account lock release failed");
        }
        result
    }

    async fn load_or_create_account_locked(
        &self,
        account: &str,
        credential: &str,
    ) -> Result<pb::AccountData, pb::Status> {
        let hash = credential_hash(account, credential);
        match load_model::<pb::AccountData>(&self.accounts, account).await {
            Ok(Some(account)) if account.credential_hash == hash => Ok(account),
            Ok(Some(_)) => Err(error_status(code::UNAUTHENTICATED, "credential mismatch")),
            Ok(None) => {
                let gid = allocate_gid(&self.redis).await.map_err(|error| {
                    xlog::error!(account, %error, "Auth gid allocation failed");
                    error_status(code::INTERNAL, "gid allocation failed")
                })?;
                let account = pb::AccountData {
                    account: account.to_string(),
                    credential_hash: hash,
                    roles: vec![pb::Role {
                        gid,
                        sid: 0,
                        name: format!("Player{gid}"),
                        level: 1,
                        icon: 0,
                    }],
                    created_at: unix_seconds(),
                };
                save_model(&self.accounts, &account).await.map_err(|error| {
                    xlog::error!(account = %account.account, %error, "Auth account create failed");
                    error_status(code::INTERNAL, "account create failed")
                })?;
                Ok(account)
            }
            Err(error) => {
                xlog::error!(account, %error, "Auth account load failed");
                Err(error_status(code::INTERNAL, "account load failed"))
            }
        }
    }

    async fn use_role(
        &self,
        _context: xframe::xhttp::RequestContext,
        request: pb::AuthUseRoleReq,
    ) -> pb::AuthUseRoleRsp {
        let Ok(_permit) = self.inflight.clone().try_acquire_owned() else {
            return use_role_error(code::OVERLOADED, "Auth request capacity exhausted");
        };
        if request.gid <= 0 || request.token.is_empty() || request.device_id.len() < 8 {
            return use_role_error(code::INVALID_ARGUMENT, "invalid role request");
        }
        match self
            .token
            .simple_token_decode(&request.token, &request.device_id)
        {
            Ok(gid) if gid == request.gid => {}
            _ => return use_role_error(code::UNAUTHENTICATED, "token verification failed"),
        }
        match load_online(&self.redis, request.gid).await {
            Ok(Some(online)) if online.token == request.token => {}
            Ok(_) => return use_role_error(code::UNAUTHENTICATED, "token state mismatch"),
            Err(error) => {
                xlog::error!(gid = request.gid, %error, "Auth online state load failed");
                return use_role_error(code::INTERNAL, "online state load failed");
            }
        }

        let gates = match self.frame.service_instances(ServiceType::Gate) {
            Ok(gates) => gates,
            Err(error) => {
                xlog::warn!(gid = request.gid, %error, "Auth Gate discovery unavailable");
                return use_role_error(code::TEMPORARILY_UNAVAILABLE, "Gate unavailable");
            }
        };
        let available = gates
            .iter()
            .filter(|gate| gate.enable && gate.healthy == ServiceStatus::Health)
            .map(|gate| (self.gate_player_capacity - gate.online_count).max(0) as i64)
            .sum::<i64>();
        let position = match enqueue_login(
            &self.redis,
            request.gid,
            unix_millis(),
            self.login_queue_entry_ttl,
        )
        .await
        {
            Ok(position) => position,
            Err(error) => {
                xlog::error!(gid = request.gid, %error, "Auth login queue failed");
                return use_role_error(code::INTERNAL, "login queue failed");
            }
        };
        if position > self.login_queue_capacity {
            let _ = leave_login_queue(&self.redis, request.gid).await;
            return use_role_error(code::OVERLOADED, "login queue is full");
        }
        if available == 0 || position > available {
            return pb::AuthUseRoleRsp {
                status: Some(ok_status()),
                gid: request.gid,
                endpoints: Vec::new(),
                queue: Some(pb::LoginQueue {
                    position,
                    next_request_time: unix_seconds() + self.login_queue_retry_seconds,
                }),
            };
        }
        match self.role_admission.allow("all").await {
            Ok(result) if result.allowed => {}
            Ok(_) => {
                return pb::AuthUseRoleRsp {
                    status: Some(ok_status()),
                    gid: request.gid,
                    endpoints: Vec::new(),
                    queue: Some(pb::LoginQueue {
                        position,
                        next_request_time: unix_seconds() + self.login_queue_retry_seconds,
                    }),
                };
            }
            Err(error) => {
                xlog::error!(gid = request.gid, %error, "Auth role admission failed");
                return use_role_error(code::INTERNAL, "role admission failed");
            }
        }

        let gate = match self.frame.pick_min_online_discovered(ServiceType::Gate) {
            Ok(gate) => gate,
            Err(error) => {
                xlog::warn!(gid = request.gid, %error, "Auth Gate selection failed");
                return use_role_error(code::TEMPORARILY_UNAVAILABLE, "Gate unavailable");
            }
        };
        let endpoints = gate_endpoints(&gate);
        if endpoints.is_empty() {
            return use_role_error(code::TEMPORARILY_UNAVAILABLE, "Gate endpoint unavailable");
        }
        if let Err(error) = leave_login_queue(&self.redis, request.gid).await {
            xlog::warn!(gid = request.gid, %error, "Auth login queue removal failed");
        }
        xlog::info!(
            gid = request.gid,
            gate_id = gate.instance_id,
            endpoints = endpoints.len(),
            "Auth role admitted"
        );
        pb::AuthUseRoleRsp {
            status: Some(ok_status()),
            gid: request.gid,
            endpoints,
            queue: None,
        }
    }
}

fn gate_endpoints(gate: &xframe::xservice::ServiceInstance) -> Vec<pb::Endpoint> {
    let mut endpoints = Vec::with_capacity(3);
    for (transport, port_key) in [
        ("tcp", "tcp_port"),
        ("kcp", "kcp_port"),
        ("websocket", "websocket_port"),
    ] {
        let Some(port) = gate
            .meta_data
            .get(port_key)
            .and_then(|port| port.parse::<u32>().ok())
        else {
            continue;
        };
        endpoints.push(pb::Endpoint {
            transport: transport.to_string(),
            host: gate.host.clone(),
            port,
            path: if transport == "websocket" {
                gate.meta_data
                    .get("websocket_path")
                    .cloned()
                    .unwrap_or_else(|| "/".to_string())
            } else {
                String::new()
            },
        });
    }
    endpoints
}

fn login_error(error_code: i32, message: &'static str) -> pb::AuthLoginRsp {
    pb::AuthLoginRsp {
        status: Some(error_status(error_code, message)),
        ..Default::default()
    }
}

fn use_role_error(error_code: i32, message: &'static str) -> pb::AuthUseRoleRsp {
    pb::AuthUseRoleRsp {
        status: Some(error_status(error_code, message)),
        ..Default::default()
    }
}

struct AuthApplication {
    cluster: String,
    redis: xframe::xredis::Client,
    service_load_interval: Duration,
    metrics_interval: Duration,
    api: AuthApi,
    service_load_task: Option<JoinHandle<()>>,
    metrics_task: Option<JoinHandle<()>>,
}

impl AuthApplication {
    fn new(
        cluster: String,
        redis: xframe::xredis::Client,
        service_load_interval: Duration,
        metrics_interval: Duration,
        api: AuthApi,
    ) -> Self {
        Self {
            cluster,
            redis,
            service_load_interval,
            metrics_interval,
            api,
            service_load_task: None,
            metrics_task: None,
        }
    }
}

impl Application for AuthApplication {
    async fn start(&mut self, frame: FrameHandle) -> ApplicationResult {
        frame.watch(self.cluster.clone(), ServiceType::Gate).await?;
        refresh_service_online(&frame, &self.redis, &self.cluster, ServiceType::Gate).await?;
        self.service_load_task = Some(spawn_service_loads(
            frame.clone(),
            self.redis.clone(),
            self.cluster.clone(),
            self.service_load_interval,
        ));
        self.metrics_task = spawn_metrics(frame, self.api.clone(), self.metrics_interval);
        Ok(())
    }

    async fn shutdown(&mut self, _frame: FrameHandle) -> ApplicationResult {
        if let Some(task) = self.service_load_task.take() {
            task.abort();
            let _ = task.await;
        }
        if let Some(task) = self.metrics_task.take() {
            task.abort();
            let _ = task.await;
        }
        Ok(())
    }
}

fn spawn_service_loads(
    frame: FrameHandle,
    redis: xframe::xredis::Client,
    cluster: String,
    interval: Duration,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(interval);
        ticker.tick().await;
        loop {
            ticker.tick().await;
            if let Err(error) =
                refresh_service_online(&frame, &redis, &cluster, ServiceType::Gate).await
            {
                xlog::warn!(%error, "Auth Gate online refresh failed");
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
        ticker.tick().await;
        loop {
            ticker.tick().await;
            let stats = frame.stats();
            xlog::info!(
                frame_state = ?stats.state,
                available_request_slots = api.inflight.available_permits(),
                rpc_pending = stats.rpc.pending,
                rpc_pending_rejected = stats.rpc.pending_rejected,
                "Auth runtime stats"
            );
        }
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn example_config_builds_http_only_auth() {
        let config = Config::parse(include_str!("../../../config/auth.yaml")).unwrap();
        let frame = frame_config(&config).unwrap();

        assert!(frame.http.is_some());
        assert!(frame.service_server.is_none());
        assert_eq!(config.runtime.service_load_interval_seconds, 3);
        assert_eq!(
            frame.node.meta_data().get("login_path").unwrap(),
            LOGIN_PATH
        );
    }

    #[test]
    fn gate_endpoints_include_every_advertised_transport() {
        let gate = xframe::xservice::ServiceInstance {
            instance_id: 1,
            healthy: ServiceStatus::Health,
            load: 0,
            online_count: 0,
            pro_version: 0,
            conf_version: 0,
            net_status: xframe::xservice::NetStatus::Invalid,
            enable: true,
            weight: 1,
            cluster_name: "local".to_string(),
            service_type: ServiceType::Gate,
            host: "gate.example".to_string(),
            port: 3201,
            update_time: String::new(),
            meta_data: HashMap::from([
                ("tcp_port".to_string(), "3201".to_string()),
                ("kcp_port".to_string(), "3202".to_string()),
                ("websocket_port".to_string(), "3203".to_string()),
                ("websocket_path".to_string(), "/ws".to_string()),
            ]),
        };

        let endpoints = gate_endpoints(&gate);

        assert_eq!(endpoints.len(), 3);
        assert_eq!(endpoints[2].path, "/ws");
    }
}
