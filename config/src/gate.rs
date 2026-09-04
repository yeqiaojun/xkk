use std::path::PathBuf;

use serde::Deserialize;

use crate::{
    GateNode, Infrastructure, LogSettings, Result, Security, ServiceVersion, invalid, load_service,
    log::LogOverride,
    parse_service,
    shared::{CommonConfig, GateNodeConfig},
};

const SERVICE: &str = "Gate";

#[derive(Debug)]
pub struct GateConfig {
    pub node: GateNode,
    pub infrastructure: Infrastructure,
    pub listeners: GateListeners,
    pub security: Security,
    pub log: LogSettings,
    pub version: ServiceVersion,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct GateRoleConfig {
    node: GateNodeConfig,
    listeners: GateListeners,
    #[serde(default)]
    log: LogOverride,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GateListeners {
    pub tcp_port: Option<u16>,
    pub kcp_port: Option<u16>,
    pub websocket_port: Option<u16>,
    pub websocket_path: String,
    pub primary_transport: GateTransport,
}

impl GateListeners {
    pub fn primary_port(&self) -> Option<u16> {
        match self.primary_transport {
            GateTransport::Tcp => self.tcp_port,
            GateTransport::Kcp => self.kcp_port,
            GateTransport::Websocket => self.websocket_port,
        }
    }

    fn validate(&self) -> Result<()> {
        if self.primary_port().is_none() {
            return Err(invalid(SERVICE, "listeners.primary_transport must be enabled"));
        }
        if [self.tcp_port, self.kcp_port, self.websocket_port].into_iter().flatten().any(|port| port == 0) {
            return Err(invalid(SERVICE, "listener ports must be positive"));
        }
        if self.tcp_port.is_some() && self.tcp_port == self.websocket_port {
            return Err(invalid(SERVICE, "TCP and WebSocket cannot share one TCP port"));
        }
        if !self.websocket_path.starts_with('/') {
            return Err(invalid(SERVICE, "listeners.websocket_path must start with '/'"));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum GateTransport {
    Tcp,
    Kcp,
    Websocket,
}

impl GateTransport {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Tcp => "tcp",
            Self::Kcp => "kcp",
            Self::Websocket => "websocket",
        }
    }
}

impl GateConfig {
    pub fn load(path: impl Into<PathBuf>) -> Result<Self> {
        let (common, role, version) = load_service(path)?;
        Self::compose(common, role, version)
    }

    pub fn parse(common_yaml: &str, role_yaml: &str, version_json: &str) -> Result<Self> {
        let (common, role, version) = parse_service(common_yaml, role_yaml, version_json)?;
        Self::compose(common, role, version)
    }

    fn compose(common: CommonConfig, role: GateRoleConfig, version: ServiceVersion) -> Result<Self> {
        common.validate(SERVICE)?;
        let config = Self {
            node: role.node.compose(common.cluster.clone()),
            infrastructure: common.infrastructure,
            listeners: role.listeners,
            security: common.security,
            log: common.log.apply(role.log),
            version,
        };
        config.validate()?;
        Ok(config)
    }

    pub fn validate(&self) -> Result<()> {
        self.node.validate(SERVICE)?;
        self.infrastructure.validate(SERVICE)?;
        self.listeners.validate()?;
        self.security.validate(SERVICE)?;
        self.log.validate(SERVICE)
    }
}
