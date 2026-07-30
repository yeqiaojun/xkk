use std::path::PathBuf;

use serde::Deserialize;

use crate::{Infrastructure, LogSettings, Result, ServiceNode, invalid, load, parse};

const SERVICE: &str = "Public";

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PublicConfig {
    pub node: ServiceNode,
    pub infrastructure: Infrastructure,
    pub storage: PublicStorage,
    pub capacity: PublicCapacity,
    pub runtime: PublicRuntime,
    pub log: LogSettings,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PublicStorage {
    pub mongo_database: String,
    pub mail_collection: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PublicCapacity {
    pub rpc_pending: usize,
    pub write_queue: usize,
    pub max_mails_per_player: usize,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PublicRuntime {
    pub rpc_timeout_ms: u64,
    pub mail_lock_seconds: u64,
    pub shutdown_drain_seconds: u64,
    pub metrics_interval_seconds: u64,
}

impl PublicConfig {
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
        self.log.validate(SERVICE)?;
        if self.storage.mongo_database.is_empty() || self.storage.mail_collection.is_empty() {
            return Err(invalid(SERVICE, "storage names must not be empty"));
        }
        if self.capacity.rpc_pending == 0
            || self.capacity.write_queue == 0
            || self.capacity.max_mails_per_player == 0
        {
            return Err(invalid(SERVICE, "capacity values must be positive"));
        }
        if self.runtime.rpc_timeout_ms == 0
            || self.runtime.mail_lock_seconds == 0
            || self.runtime.shutdown_drain_seconds == 0
        {
            return Err(invalid(SERVICE, "runtime timeouts must be positive"));
        }
        Ok(())
    }
}
