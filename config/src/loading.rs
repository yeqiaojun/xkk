use std::{
    env, io,
    path::{Path, PathBuf},
};

use serde::de::DeserializeOwned;
use thiserror::Error;

use crate::{ServiceVersion, shared::CommonConfig};

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
    #[error("decode configuration version {path}: {source}")]
    DecodeVersion {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
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
    let yaml = read(&path)?;
    decode(&yaml, path)
}

pub(crate) fn load_service<T>(
    role_path: impl Into<PathBuf>,
) -> Result<(CommonConfig, T, ServiceVersion)>
where
    T: DeserializeOwned,
{
    let role_path = role_path.into();
    let directory = role_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .to_path_buf();
    let common = load(directory.join("common.yaml"))?;
    let role = load(&role_path)?;
    let version_path = directory.join("version.json");
    let json = read(&version_path)?;
    let version = ServiceVersion::decode(&json, version_path)?;
    Ok((common, role, version))
}

pub(crate) fn parse_service<T>(
    common_yaml: &str,
    role_yaml: &str,
    version_json: &str,
) -> Result<(CommonConfig, T, ServiceVersion)>
where
    T: DeserializeOwned,
{
    let common = parse(common_yaml)?;
    let role = parse(role_yaml)?;
    let version = ServiceVersion::decode(version_json, PathBuf::from("<inline-version>"))?;
    Ok((common, role, version))
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

fn read(path: &Path) -> Result<String> {
    std::fs::read_to_string(path).map_err(|source| ConfigError::Read {
        path: path.to_path_buf(),
        source,
    })
}

pub(crate) fn invalid(service: &'static str, message: &'static str) -> ConfigError {
    ConfigError::Invalid { service, message }
}
