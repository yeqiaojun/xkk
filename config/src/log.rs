use std::path::PathBuf;

use serde::Deserialize;

use crate::{Result, invalid};

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LogSettings {
    pub level: LogLevel,
    pub stdout: bool,
    pub file: Option<PathBuf>,
    pub async_queue_capacity: usize,
    pub rotation: LogRotation,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct LogOverride {
    pub level: Option<LogLevel>,
    pub stdout: Option<bool>,
    pub file: Option<PathBuf>,
    pub async_queue_capacity: Option<usize>,
    pub rotation: Option<LogRotation>,
}

impl LogSettings {
    pub(crate) fn apply(mut self, overrides: LogOverride) -> Self {
        if let Some(level) = overrides.level {
            self.level = level;
        }
        if let Some(stdout) = overrides.stdout {
            self.stdout = stdout;
        }
        if let Some(file) = overrides.file {
            self.file = Some(file);
        }
        if let Some(capacity) = overrides.async_queue_capacity {
            self.async_queue_capacity = capacity;
        }
        if let Some(rotation) = overrides.rotation {
            self.rotation = rotation;
        }
        self
    }

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
            stdout: self.stdout.then_some(xlog::Format::Console),
            file: self.file.as_ref().map(|_| xlog::Format::Console),
            file_path: self.file.clone().unwrap_or_else(|| default_file.into()),
            async_queue_capacity: self.async_queue_capacity,
            rotation: self.rotation.into(),
            ..xlog::Options::default()
        }
    }
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum LogLevel {
    Trace,
    Debug,
    Info,
    Warn,
    Error,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum LogRotation {
    Daily,
    Hourly,
}

impl From<LogRotation> for xlog::Rotation {
    fn from(rotation: LogRotation) -> Self {
        match rotation {
            LogRotation::Daily => Self::Daily,
            LogRotation::Hourly => Self::Hourly,
        }
    }
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
