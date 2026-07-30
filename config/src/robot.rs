use std::path::PathBuf;

use serde::Deserialize;

use crate::{Result, invalid, load, parse};

const SERVICE: &str = "xkk-robot";

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RobotConfig {
    pub auth_url: String,
    pub account: String,
    pub credential: String,
    pub device_id: String,
    pub platform: String,
    pub client_version: String,
    pub transport: RobotTransport,
    pub timeout_seconds: u64,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum RobotTransport {
    Tcp,
    Kcp,
    Websocket,
}

impl RobotTransport {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Tcp => "tcp",
            Self::Kcp => "kcp",
            Self::Websocket => "websocket",
        }
    }
}

impl RobotConfig {
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
        if !self.auth_url.starts_with("http://") {
            return Err(invalid(SERVICE, "auth_url must start with http://"));
        }
        if self.account.is_empty() || self.account.len() > 64 {
            return Err(invalid(SERVICE, "account length must be 1..=64"));
        }
        if self.credential.is_empty() || self.credential.len() > 256 {
            return Err(invalid(SERVICE, "credential length must be 1..=256"));
        }
        if self.device_id.len() < 8 {
            return Err(invalid(SERVICE, "device_id must contain at least 8 bytes"));
        }
        if self.platform.is_empty() || self.client_version.is_empty() {
            return Err(invalid(
                SERVICE,
                "platform and client_version must not be empty",
            ));
        }
        if self.timeout_seconds == 0 {
            return Err(invalid(SERVICE, "timeout_seconds must be positive"));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn example_config_is_valid() {
        let config = RobotConfig::parse(include_str!("../robot.yaml")).unwrap();
        assert_eq!(config.transport, RobotTransport::Tcp);
        assert_eq!(config.timeout_seconds, 10);
    }

    #[test]
    fn invalid_device_fails_fast() {
        let yaml = include_str!("../robot.yaml").replace("xkk-robot-device", "short");
        assert!(matches!(
            RobotConfig::parse(&yaml),
            Err(crate::ConfigError::Invalid { .. })
        ));
    }
}
