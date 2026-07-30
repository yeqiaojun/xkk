use std::{collections::HashMap, path::PathBuf};

use serde::Deserialize;

use crate::{Result, invalid};

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HttpNode {
    pub cluster: String,
    pub instance_id: i32,
    pub advertise_host: String,
    pub listen_host: String,
    pub http_port: u16,
    pub pro_version: i32,
    pub conf_version: i32,
}

impl HttpNode {
    pub(crate) fn validate(&self, service: &'static str) -> Result<()> {
        validate_node(
            service,
            &self.cluster,
            self.instance_id,
            &self.advertise_host,
            &self.listen_host,
        )?;
        if self.http_port == 0 {
            return Err(invalid(service, "node.http_port must be positive"));
        }
        Ok(())
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ServiceNode {
    pub cluster: String,
    pub instance_id: i32,
    pub advertise_host: String,
    pub listen_host: String,
    pub service_port: u16,
    pub pro_version: i32,
    pub conf_version: i32,
}

impl ServiceNode {
    pub(crate) fn validate(&self, service: &'static str) -> Result<()> {
        validate_node(
            service,
            &self.cluster,
            self.instance_id,
            &self.advertise_host,
            &self.listen_host,
        )?;
        if self.service_port == 0 {
            return Err(invalid(service, "node.service_port must be positive"));
        }
        Ok(())
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GateNode {
    pub cluster: String,
    pub instance_id: i32,
    pub advertise_host: String,
    pub listen_host: String,
    pub pro_version: i32,
    pub conf_version: i32,
}

impl GateNode {
    pub(crate) fn validate(&self, service: &'static str) -> Result<()> {
        validate_node(
            service,
            &self.cluster,
            self.instance_id,
            &self.advertise_host,
            &self.listen_host,
        )
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Infrastructure {
    pub etcd_dsn: String,
    pub mongo_dsn: String,
    pub redis_dsn: String,
    pub etcd_lease_ttl_seconds: u64,
}

impl Infrastructure {
    pub(crate) fn validate(&self, service: &'static str) -> Result<()> {
        if self.etcd_dsn.is_empty() || self.mongo_dsn.is_empty() || self.redis_dsn.is_empty() {
            return Err(invalid(service, "infrastructure DSNs must not be empty"));
        }
        if self.etcd_lease_ttl_seconds == 0 {
            return Err(invalid(
                service,
                "infrastructure.etcd_lease_ttl_seconds must be positive",
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Security {
    pub token_secret: String,
    pub token_expire_seconds: i64,
}

impl Security {
    pub(crate) fn validate(&self, service: &'static str) -> Result<()> {
        if self.token_secret.is_empty() || self.token_expire_seconds <= 0 {
            return Err(invalid(service, "security token settings are invalid"));
        }
        Ok(())
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LogSettings {
    pub level: LogLevel,
    #[serde(default)]
    pub targets: HashMap<String, LogLevel>,
    pub stdout: bool,
    pub file: Option<PathBuf>,
    pub async_queue_capacity: usize,
}

impl LogSettings {
    pub(crate) fn validate(&self, service: &'static str) -> Result<()> {
        if self.async_queue_capacity == 0 {
            return Err(invalid(
                service,
                "log.async_queue_capacity must be positive",
            ));
        }
        Ok(())
    }

    pub fn options(&self, default_file: impl Into<PathBuf>) -> xlog::Options {
        xlog::Options {
            level: self.level.into(),
            target_levels: self
                .targets
                .iter()
                .map(|(target, level)| (target.clone(), (*level).into()))
                .collect(),
            stdout: self.stdout.then_some(xlog::Format::Console),
            file: self.file.as_ref().map(|_| xlog::Format::Console),
            file_path: self.file.clone().unwrap_or_else(|| default_file.into()),
            async_queue_capacity: self.async_queue_capacity,
            ..xlog::Options::default()
        }
    }
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LogLevel {
    Trace,
    Debug,
    Info,
    Warn,
    Error,
}

impl From<LogLevel> for xlog::Level {
    fn from(level: LogLevel) -> Self {
        match level {
            LogLevel::Trace => Self::Trace,
            LogLevel::Debug => Self::Debug,
            LogLevel::Info => Self::Info,
            LogLevel::Warn => Self::Warn,
            LogLevel::Error => Self::Error,
        }
    }
}

fn validate_node(
    service: &'static str,
    cluster: &str,
    instance_id: i32,
    advertise_host: &str,
    listen_host: &str,
) -> Result<()> {
    if cluster.is_empty() || advertise_host.is_empty() || listen_host.is_empty() {
        return Err(invalid(service, "node names and hosts must not be empty"));
    }
    if instance_id <= 0 {
        return Err(invalid(service, "node.instance_id must be positive"));
    }
    Ok(())
}
