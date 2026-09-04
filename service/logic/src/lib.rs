mod persistence;
mod player;
mod runtime;
pub mod service;
mod stats;

pub use persistence::{LogicState, Persistence, SavePlayer};
pub use runtime::{Completed, LogicCall, LogicCallError, LogicConfig, LogicRuntime, RejectReason, RuntimeState, ShutdownError};
pub use service::{Config, ServiceError, config_path, run};
pub use stats::LogicStats;
pub use xkk_common::LatencyStats;

#[cfg(test)]
mod tests;
