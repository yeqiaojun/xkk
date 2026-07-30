use std::path::PathBuf;

use serde::Deserialize;

use crate::{Infrastructure, LogSettings, Result, ServiceNode, invalid, load, parse};

const SERVICE: &str = "Logic";

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LogicConfig {
    pub node: ServiceNode,
    pub infrastructure: Infrastructure,
    pub storage: LogicStorage,
    pub capacity: LogicCapacity,
    pub runtime: LogicRuntime,
    pub log: LogSettings,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LogicStorage {
    pub mongo_database: String,
    pub player_collection: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LogicCapacity {
    pub rpc_pending: usize,
    pub write_queue: usize,
    pub resident_players: usize,
    pub max_dirty_players: usize,
    pub max_inflight_calls: usize,
    pub max_inflight_kib: usize,
    pub max_calls_per_gid: usize,
    pub max_kib_per_gid: usize,
    pub mailbox_shards: usize,
    pub batch_save_count: usize,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LogicRuntime {
    pub player_ttl_seconds: u64,
    pub rpc_timeout_ms: u64,
    pub shutdown_drain_seconds: u64,
    pub metrics_interval_seconds: u64,
}

impl LogicConfig {
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
        if self.storage.mongo_database.is_empty() || self.storage.player_collection.is_empty() {
            return Err(invalid(SERVICE, "storage names must not be empty"));
        }
        if self.capacity.rpc_pending == 0
            || self.capacity.write_queue == 0
            || self.capacity.resident_players == 0
            || self.capacity.max_dirty_players == 0
            || self.capacity.max_inflight_calls == 0
            || self.capacity.max_inflight_kib == 0
            || self.capacity.max_calls_per_gid == 0
            || self.capacity.max_kib_per_gid == 0
            || self.capacity.batch_save_count == 0
        {
            return Err(invalid(SERVICE, "capacity values must be positive"));
        }
        if !self.capacity.mailbox_shards.is_power_of_two() {
            return Err(invalid(
                SERVICE,
                "capacity.mailbox_shards must be a power of two",
            ));
        }
        if self.runtime.player_ttl_seconds == 0
            || self.runtime.rpc_timeout_ms == 0
            || self.runtime.shutdown_drain_seconds == 0
        {
            return Err(invalid(SERVICE, "runtime timeouts must be positive"));
        }
        Ok(())
    }
}
