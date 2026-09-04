use serde::Deserialize;

use crate::{LogSettings, Result, invalid};

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CommonConfig {
    pub cluster: String,
    pub infrastructure: Infrastructure,
    pub security: Security,
    pub log: LogSettings,
}

impl CommonConfig {
    pub(crate) fn validate(&self, service: &'static str) -> Result<()> {
        if self.cluster.is_empty() {
            return Err(invalid(service, "cluster must not be empty"));
        }
        self.infrastructure.validate(service)?;
        self.security.validate(service)?;
        self.log.validate(service)
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HttpNode {
    pub cluster: String,
    pub instance_id: i32,
    pub advertise_host: String,
    pub listen_host: String,
    pub http_port: u16,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct HttpNodeConfig {
    pub instance_id: i32,
    pub advertise_host: String,
    pub listen_host: String,
    pub http_port: u16,
}

impl HttpNodeConfig {
    pub(crate) fn compose(self, cluster: String) -> HttpNode {
        HttpNode {
            cluster,
            instance_id: self.instance_id,
            advertise_host: self.advertise_host,
            listen_host: self.listen_host,
            http_port: self.http_port,
        }
    }
}

impl HttpNode {
    pub(crate) fn validate(&self, service: &'static str) -> Result<()> {
        validate_node(service, &self.cluster, self.instance_id, &self.advertise_host, &self.listen_host)?;
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
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ServiceNodeConfig {
    pub instance_id: i32,
    pub advertise_host: String,
    pub listen_host: String,
    pub service_port: u16,
}

impl ServiceNodeConfig {
    pub(crate) fn compose(self, cluster: String) -> ServiceNode {
        ServiceNode {
            cluster,
            instance_id: self.instance_id,
            advertise_host: self.advertise_host,
            listen_host: self.listen_host,
            service_port: self.service_port,
        }
    }
}

impl ServiceNode {
    pub(crate) fn validate(&self, service: &'static str) -> Result<()> {
        validate_node(service, &self.cluster, self.instance_id, &self.advertise_host, &self.listen_host)?;
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
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct GateNodeConfig {
    pub instance_id: i32,
    pub advertise_host: String,
    pub listen_host: String,
}

impl GateNodeConfig {
    pub(crate) fn compose(self, cluster: String) -> GateNode {
        GateNode { cluster, instance_id: self.instance_id, advertise_host: self.advertise_host, listen_host: self.listen_host }
    }
}

impl GateNode {
    pub(crate) fn validate(&self, service: &'static str) -> Result<()> {
        validate_node(service, &self.cluster, self.instance_id, &self.advertise_host, &self.listen_host)
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Infrastructure {
    pub etcd_dsn: String,
    pub mongo_dsn: String,
    pub redis_dsn: String,
}

impl Infrastructure {
    pub(crate) fn validate(&self, service: &'static str) -> Result<()> {
        if self.etcd_dsn.is_empty() || self.mongo_dsn.is_empty() || self.redis_dsn.is_empty() {
            return Err(invalid(service, "infrastructure DSNs must not be empty"));
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

fn validate_node(service: &'static str, cluster: &str, instance_id: i32, advertise_host: &str, listen_host: &str) -> Result<()> {
    if cluster.is_empty() || advertise_host.is_empty() || listen_host.is_empty() {
        return Err(invalid(service, "node names and hosts must not be empty"));
    }
    if instance_id <= 0 {
        return Err(invalid(service, "node.instance_id must be positive"));
    }
    Ok(())
}
