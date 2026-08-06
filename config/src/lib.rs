mod auth;
mod gate;
mod loading;
mod log;
mod logic;
mod public;
mod query;
mod robot;
mod shared;
mod version;

pub use auth::AuthConfig;
pub use gate::{GateConfig, GateListeners, GateTransport};
pub use loading::{ConfigError, Result, config_path};
pub use log::{LogLevel, LogRotation, LogSettings};
pub use logic::LogicConfig;
pub use public::PublicConfig;
pub use query::QueryConfig;
pub use robot::{RobotConfig, RobotTransport};
pub use shared::{GateNode, HttpNode, Infrastructure, Security, ServiceNode};
pub use version::ServiceVersion;

pub(crate) use loading::{invalid, load, load_service, parse, parse_service};

#[cfg(test)]
mod tests;
