use std::{sync::Arc, time::Duration};

use tokio::sync::Semaphore;
use xframe::{
    FrameHandle, ServiceType,
    xmongo::{self, mongodb::bson::Document},
    xservice::ServiceStatus,
};
use xkk_cache::{allocate_gid, enqueue_login, leave_login_queue, load_online, set_token};
use xkk_common::{credential_hash, unix_millis, unix_seconds};
use xkk_config::AuthConfig;
use xkk_persist::{load_model, save_model};
use xkk_protocol::{code, error_status, ok_status, pb};
use xtoken::TokenCoder;

pub(crate) const LOGIN_PATH: &str = "/v1/auth/login";
pub(crate) const USE_ROLE_PATH: &str = "/v1/auth/use-role";

#[derive(Clone)]
pub(crate) struct AuthApi {
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
    pub(crate) fn new(
        frame: FrameHandle,
        mongo: xmongo::Client,
        redis: xframe::xredis::Client,
        config: &AuthConfig,
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

    pub(crate) fn available_request_slots(&self) -> usize {
        self.inflight.available_permits()
    }

    pub(crate) async fn login(
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

    pub(crate) async fn use_role(
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

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

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
