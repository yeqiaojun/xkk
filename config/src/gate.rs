use std::path::PathBuf;

use serde::Deserialize;

use crate::{GateNode, Infrastructure, LogSettings, Result, Security, invalid, load, parse};

const SERVICE: &str = "Gate";

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GateConfig {
    pub node: GateNode,
    pub infrastructure: Infrastructure,
    pub listeners: GateListeners,
    pub capacity: GateCapacity,
    pub runtime: GateRuntime,
    pub security: Security,
    pub log: LogSettings,
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

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GateCapacity {
    pub rpc_pending: usize,
    pub write_queue: usize,
    pub max_external_handshakes: usize,
    pub max_external_connections: usize,
    pub client_mailbox: usize,
    pub shutdown_cleanup_concurrency: usize,
    pub outbox_messages: usize,
    pub client_request_window_count: usize,
    pub client_burst_count: usize,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GateRuntime {
    pub shutdown_drain_seconds: u64,
    pub metrics_interval_seconds: u64,
    pub rpc_timeout_ms: u64,
    pub resume_seconds: u64,
    pub reconnect_total: usize,
    pub reconnect_window_seconds: u64,
    pub reconnect_window_count: usize,
    pub client_request_window_ms: u64,
    pub client_burst_window_ms: u64,
}

impl GateConfig {
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

        if self.listeners.primary_port().is_none() {
            return Err(invalid(
                SERVICE,
                "listeners.primary_transport must be enabled",
            ));
        }
        if [
            self.listeners.tcp_port,
            self.listeners.kcp_port,
            self.listeners.websocket_port,
        ]
        .into_iter()
        .flatten()
        .any(|port| port == 0)
        {
            return Err(invalid(SERVICE, "listener ports must be positive"));
        }
        if self.listeners.tcp_port.is_some()
            && self.listeners.tcp_port == self.listeners.websocket_port
        {
            return Err(invalid(
                SERVICE,
                "TCP and WebSocket cannot share one TCP port",
            ));
        }
        if !self.listeners.websocket_path.starts_with('/') {
            return Err(invalid(
                SERVICE,
                "listeners.websocket_path must start with '/'",
            ));
        }
        if self.capacity.rpc_pending == 0
            || self.capacity.write_queue == 0
            || self.capacity.max_external_handshakes == 0
            || self.capacity.max_external_connections == 0
            || self.capacity.client_mailbox == 0
            || self.capacity.shutdown_cleanup_concurrency == 0
            || self.capacity.outbox_messages == 0
            || self.capacity.client_request_window_count == 0
            || self.capacity.client_burst_count == 0
        {
            return Err(invalid(SERVICE, "capacity values must be positive"));
        }
        if self.runtime.shutdown_drain_seconds == 0
            || self.runtime.rpc_timeout_ms == 0
            || self.runtime.resume_seconds == 0
            || self.runtime.reconnect_total == 0
            || self.runtime.reconnect_window_seconds == 0
            || self.runtime.reconnect_window_count == 0
            || self.runtime.client_request_window_ms == 0
            || self.runtime.client_burst_window_ms == 0
        {
            return Err(invalid(SERVICE, "runtime windows must be positive"));
        }
        if self.capacity.client_burst_count > self.capacity.client_request_window_count {
            return Err(invalid(
                SERVICE,
                "capacity.client_burst_count cannot exceed the long-window limit",
            ));
        }
        if self.runtime.reconnect_window_count > self.runtime.reconnect_total {
            return Err(invalid(
                SERVICE,
                "runtime.reconnect_window_count cannot exceed reconnect_total",
            ));
        }
        Ok(())
    }
}
