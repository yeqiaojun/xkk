use std::path::PathBuf;

use serde::Deserialize;

use crate::{ConfigError, Result, invalid};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ServiceVersion {
    pub program: i32,
    pub conf: i32,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct VersionDocument {
    conf_version: i32,
}

impl ServiceVersion {
    pub(crate) fn decode(json: &str, path: PathBuf) -> Result<Self> {
        let document: VersionDocument = serde_json::from_str(json).map_err(|source| ConfigError::DecodeVersion { path, source })?;
        if document.conf_version < 0 {
            return Err(invalid("version", "conf_version must be non-negative"));
        }
        Ok(Self { program: env!("XKK_PRO_VERSION").parse().expect("build script validates XKK_PRO_VERSION"), conf: document.conf_version })
    }
}
