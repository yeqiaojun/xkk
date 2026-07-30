mod auth;
mod gate;
mod logic;
mod public;
mod query;
mod robot;
mod shared;

use std::{env, io, path::PathBuf};

use serde::de::DeserializeOwned;
use thiserror::Error;

pub use auth::{AuthCapacity, AuthConfig, AuthRuntime, AuthStorage};
pub use gate::{GateCapacity, GateConfig, GateListeners, GateRuntime, GateTransport};
pub use logic::{LogicCapacity, LogicConfig, LogicRuntime, LogicStorage};
pub use public::{PublicCapacity, PublicConfig, PublicRuntime, PublicStorage};
pub use query::{QueryCapacity, QueryConfig, QueryRuntime, QueryStorage};
pub use robot::{RobotConfig, RobotTransport};
pub use shared::{
    GateNode, HttpNode, Infrastructure, LogLevel, LogSettings, Security, ServiceNode,
};

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("usage: {service} --config <path>")]
    Usage { service: &'static str },
    #[error("read configuration {path}: {source}")]
    Read {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("decode configuration {path}: {source}")]
    Decode {
        path: PathBuf,
        #[source]
        source: serde_yaml::Error,
    },
    #[error("invalid {service} configuration: {message}")]
    Invalid {
        service: &'static str,
        message: &'static str,
    },
}

pub type Result<T> = std::result::Result<T, ConfigError>;

pub fn config_path(service: &'static str) -> Result<PathBuf> {
    let mut args = env::args_os().skip(1);
    match (args.next(), args.next(), args.next()) {
        (Some(flag), Some(path), None) if flag == "--config" => Ok(path.into()),
        _ => Err(ConfigError::Usage { service }),
    }
}

pub(crate) fn load<T>(path: impl Into<PathBuf>) -> Result<T>
where
    T: DeserializeOwned,
{
    let path = path.into();
    let yaml = std::fs::read_to_string(&path).map_err(|source| ConfigError::Read {
        path: path.clone(),
        source,
    })?;
    decode(&yaml, path)
}

pub(crate) fn parse<T>(yaml: &str) -> Result<T>
where
    T: DeserializeOwned,
{
    decode(yaml, PathBuf::from("<inline>"))
}

fn decode<T>(yaml: &str, path: PathBuf) -> Result<T>
where
    T: DeserializeOwned,
{
    serde_yaml::from_str(yaml).map_err(|source| ConfigError::Decode { path, source })
}

pub(crate) fn invalid(service: &'static str, message: &'static str) -> ConfigError {
    ConfigError::Invalid { service, message }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn example_configs_decode_and_validate() {
        AuthConfig::parse(include_str!("../auth.yaml")).unwrap();
        GateConfig::parse(include_str!("../gate.yaml")).unwrap();
        LogicConfig::parse(include_str!("../logic.yaml")).unwrap();
        PublicConfig::parse(include_str!("../public.yaml")).unwrap();
        QueryConfig::parse(include_str!("../query.yaml")).unwrap();
        RobotConfig::parse(include_str!("../robot.yaml")).unwrap();
    }

    #[test]
    fn unknown_fields_fail_fast() {
        let yaml = include_str!("../query.yaml").replace(
            "  max_gamer_ids: 100",
            "  max_gamer_ids: 100\n  hidden_budget: 1",
        );
        assert!(matches!(
            QueryConfig::parse(&yaml),
            Err(ConfigError::Decode { .. })
        ));
    }
}
