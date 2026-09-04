use std::path::PathBuf;

use serde::Deserialize;

use crate::{
    HttpNode, Infrastructure, LogSettings, Result, ServiceVersion, load_service,
    log::LogOverride,
    parse_service,
    shared::{CommonConfig, HttpNodeConfig},
};

const SERVICE: &str = "Query";

#[derive(Debug)]
pub struct QueryConfig {
    pub node: HttpNode,
    pub infrastructure: Infrastructure,
    pub log: LogSettings,
    pub version: ServiceVersion,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct QueryRoleConfig {
    node: HttpNodeConfig,
    #[serde(default)]
    log: LogOverride,
}

impl QueryConfig {
    pub fn load(path: impl Into<PathBuf>) -> Result<Self> {
        let (common, role, version) = load_service(path)?;
        Self::compose(common, role, version)
    }

    pub fn parse(common_yaml: &str, role_yaml: &str, version_json: &str) -> Result<Self> {
        let (common, role, version) = parse_service(common_yaml, role_yaml, version_json)?;
        Self::compose(common, role, version)
    }

    fn compose(common: CommonConfig, role: QueryRoleConfig, version: ServiceVersion) -> Result<Self> {
        common.validate(SERVICE)?;
        let config = Self {
            node: role.node.compose(common.cluster.clone()),
            infrastructure: common.infrastructure,
            log: common.log.apply(role.log),
            version,
        };
        config.validate()?;
        Ok(config)
    }

    pub fn validate(&self) -> Result<()> {
        self.node.validate(SERVICE)?;
        self.infrastructure.validate(SERVICE)?;
        self.log.validate(SERVICE)
    }
}
