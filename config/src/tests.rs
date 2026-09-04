use std::path::PathBuf;

use super::*;

#[test]
fn common_and_auth_role_compose_through_the_public_interface() {
    let common = r#"
cluster: local
infrastructure:
  etcd_dsn: etcd://127.0.0.1:2379
  mongo_dsn: mongodb://127.0.0.1:27017/xkk
  redis_dsn: redis://127.0.0.1:6379
security:
  token_secret: local-secret
  token_expire_seconds: 604800
log:
  level: info
  stdout: true
  rotation: daily
  async_queue_capacity: 8192
"#;
    let auth = r#"
node:
  instance_id: 1
  advertise_host: 127.0.0.1
  listen_host: 0.0.0.0
  http_port: 3501
log:
  level: debug
  file: logs/auth.log
  rotation: hourly
"#;

    let config = AuthConfig::parse(common, auth, r#"{"conf_version":7}"#).unwrap();

    assert_eq!(config.node.cluster, "local");
    assert_eq!(config.version.conf, 7);
    assert_eq!(config.log.level, LogLevel::Debug);
    assert_eq!(config.log.rotation, LogRotation::Hourly);
    assert_eq!(config.log.options("unused.log").rotation, xlog::Rotation::Hourly);
}

#[test]
fn all_example_service_configs_compose() {
    let common = include_str!("../common.yaml");
    let version = include_str!("../version.json");

    AuthConfig::parse(common, include_str!("../auth.yaml"), version).unwrap();
    GateConfig::parse(common, include_str!("../gate.yaml"), version).unwrap();
    LogicConfig::parse(common, include_str!("../logic.yaml"), version).unwrap();
    PublicConfig::parse(common, include_str!("../public.yaml"), version).unwrap();
    QueryConfig::parse(common, include_str!("../query.yaml"), version).unwrap();
    RobotConfig::parse(include_str!("../robot.yaml")).unwrap();
}

#[test]
fn explicit_role_file_loads_common_and_version_from_its_directory() {
    let role = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("auth.yaml");

    let config = AuthConfig::load(role).unwrap();

    assert_eq!(config.node.cluster, "local");
    assert_eq!(config.log.level, LogLevel::Debug);
    assert_eq!(config.version.conf, 0);
}

#[test]
fn invalid_version_document_fails_fast() {
    assert!(matches!(
        AuthConfig::parse(include_str!("../common.yaml"), include_str!("../auth.yaml"), r#"{"conf_version":-1}"#,),
        Err(ConfigError::Invalid { .. })
    ));
}

#[test]
fn retired_storage_section_fails_fast() {
    let role = format!("{}\nstorage:\n  mongo_database: xkk\n  account_collection: accounts\n", include_str!("../auth.yaml"));

    assert!(matches!(
        AuthConfig::parse(include_str!("../common.yaml"), &role, include_str!("../version.json"),),
        Err(ConfigError::Decode { .. })
    ));
}

#[test]
fn retired_runtime_and_capacity_sections_fail_fast() {
    for retired in [
        "capacity:\n  rpc_pending: 1\n",
        "runtime:\n  shutdown_drain_seconds: 1\n",
        "rpc:\n  pending_capacity: 1\n",
        "service_transport:\n  write_queue_capacity: 1\n",
        "client_transport:\n  max_connections: 1\n",
        "client_runtime:\n  mailbox_capacity: 1\n",
        "session:\n  outbox_capacity: 1\n",
        "lifecycle:\n  shutdown_drain_seconds: 1\n",
        "service_load:\n  publish_interval_seconds: 1\n",
        "telemetry:\n  metrics: {}\n",
        "mail:\n  max_per_player: 1\n",
        "player_runtime:\n  resident_capacity: 1\n",
        "player_persistence:\n  max_dirty_players: 1\n",
        "http:\n  max_body_bytes: 1\n",
        "gamer_query:\n  max_ids_per_request: 1\n",
    ] {
        let role = format!("{}\n{retired}", include_str!("../auth.yaml"));
        assert!(matches!(
            AuthConfig::parse(include_str!("../common.yaml"), &role, include_str!("../version.json"),),
            Err(ConfigError::Decode { .. })
        ));
    }
}

#[test]
fn unknown_nested_fields_fail_fast() {
    let yaml = include_str!("../query.yaml").replace("  http_port: 3401", "  http_port: 3401\n  hidden_budget: 1");
    assert!(matches!(
        QueryConfig::parse(include_str!("../common.yaml"), &yaml, include_str!("../version.json")),
        Err(ConfigError::Decode { .. })
    ));
}
