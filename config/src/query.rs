use std::path::PathBuf;

use serde::Deserialize;

use crate::{HttpNode, Infrastructure, LogSettings, Result, invalid, load, parse};

const SERVICE: &str = "Query";

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QueryConfig {
    pub node: HttpNode,
    pub infrastructure: Infrastructure,
    pub storage: QueryStorage,
    pub capacity: QueryCapacity,
    pub runtime: QueryRuntime,
    pub log: LogSettings,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QueryStorage {
    pub mongo_database: String,
    pub player_collection: String,
    pub manifest_collection: String,
    pub current_manifest_version: String,
    pub current_manifest_key: String,
    pub current_manifest_base_url: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QueryCapacity {
    pub rpc_pending: usize,
    pub max_http_body_bytes: usize,
    pub max_inflight_requests: usize,
    pub max_gamer_ids: usize,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QueryRuntime {
    pub shutdown_drain_seconds: u64,
    pub metrics_interval_seconds: u64,
}

impl QueryConfig {
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
        if self.storage.mongo_database.is_empty()
            || self.storage.player_collection.is_empty()
            || self.storage.manifest_collection.is_empty()
            || self.storage.current_manifest_version.is_empty()
            || self.storage.current_manifest_key.is_empty()
            || self.storage.current_manifest_base_url.is_empty()
        {
            return Err(invalid(SERVICE, "storage settings must not be empty"));
        }
        if self.capacity.rpc_pending == 0
            || self.capacity.max_http_body_bytes == 0
            || self.capacity.max_inflight_requests == 0
            || self.capacity.max_gamer_ids == 0
        {
            return Err(invalid(SERVICE, "capacity values must be positive"));
        }
        if self.runtime.shutdown_drain_seconds == 0 {
            return Err(invalid(
                SERVICE,
                "runtime.shutdown_drain_seconds must be positive",
            ));
        }
        Ok(())
    }
}
