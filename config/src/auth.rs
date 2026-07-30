use std::path::PathBuf;

use serde::Deserialize;

use crate::{HttpNode, Infrastructure, LogSettings, Result, Security, invalid, load, parse};

const SERVICE: &str = "Auth";

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuthConfig {
    pub node: HttpNode,
    pub infrastructure: Infrastructure,
    pub storage: AuthStorage,
    pub capacity: AuthCapacity,
    pub runtime: AuthRuntime,
    pub security: Security,
    pub log: LogSettings,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuthStorage {
    pub mongo_database: String,
    pub account_collection: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuthCapacity {
    pub rpc_pending: usize,
    pub max_http_body_bytes: usize,
    pub max_inflight_requests: usize,
    pub login_global_limit: i64,
    pub login_per_ip_limit: i64,
    pub role_admission_limit: i64,
    pub gate_player_capacity: i32,
    pub login_queue_capacity: i64,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuthRuntime {
    pub login_rate_window_ms: u64,
    pub role_admission_window_ms: u64,
    pub login_queue_retry_seconds: i64,
    pub login_queue_entry_ttl_seconds: u64,
    pub account_lock_seconds: u64,
    pub shutdown_drain_seconds: u64,
    pub metrics_interval_seconds: u64,
}

impl AuthConfig {
    pub fn load(path: impl Into<PathBuf>) -> Result<Self> {
        let config: Self = load(path)?;
        config.validate()?;
        Ok(config)
    }

    pub fn parse(yaml: &str) -> Result<Self> {
        let config: Self = parse(yaml)?;
        config.validate()?;
        Ok(config)
    }

    pub fn validate(&self) -> Result<()> {
        self.node.validate(SERVICE)?;
        self.infrastructure.validate(SERVICE)?;
        self.security.validate(SERVICE)?;
        self.log.validate(SERVICE)?;
        if self.storage.mongo_database.is_empty() || self.storage.account_collection.is_empty() {
            return Err(invalid(SERVICE, "storage names must not be empty"));
        }
        if self.capacity.rpc_pending == 0
            || self.capacity.max_http_body_bytes == 0
            || self.capacity.max_inflight_requests == 0
            || self.capacity.login_global_limit <= 0
            || self.capacity.login_per_ip_limit <= 0
            || self.capacity.role_admission_limit <= 0
            || self.capacity.gate_player_capacity <= 0
            || self.capacity.login_queue_capacity <= 0
        {
            return Err(invalid(SERVICE, "capacity values must be positive"));
        }
        if self.runtime.login_rate_window_ms == 0
            || self.runtime.role_admission_window_ms == 0
            || self.runtime.login_queue_retry_seconds <= 0
            || self.runtime.login_queue_entry_ttl_seconds == 0
            || self.runtime.account_lock_seconds == 0
            || self.runtime.shutdown_drain_seconds == 0
        {
            return Err(invalid(SERVICE, "runtime windows must be positive"));
        }
        Ok(())
    }
}
