use std::{
    collections::HashMap,
    io,
    path::PathBuf,
    sync::{Arc, atomic::AtomicI32},
    time::{Duration, Instant},
};

use thiserror::Error;
use tokio::task::JoinHandle;
use xframe::{
    Application, ApplicationResult, DiscoveryConfig, FrameConfig, FrameHandle, NodeConfig,
    RpcConfig, ServiceType, ShutdownConfig,
};
use xkk_cache::{
    delete_service_online, load_service_online_counts, publish_service_online, service_online_ttl,
};

use crate::{
    gateway::{Gateway, GatewaySettings},
    session::{ClientSessions, SessionConfig},
};
pub use xkk_config::GateConfig as Config;

// One Gate process has fixed transport and lifecycle budgets. Reaching these bounds rejects new
// work and is surfaced by the event path or the periodic overload counters below.
const SERVICE_WRITE_QUEUE_CAPACITY: usize = 1_024;
const MAX_EXTERNAL_HANDSHAKES: usize = 256;
const MAX_EXTERNAL_CONNECTIONS: usize = 65_536;
const RPC_PENDING_CAPACITY: usize = 100_000;
const SHUTDOWN_DRAIN_TIMEOUT: Duration = Duration::from_secs(10);
const SERVICE_LOAD_PUBLISH_INTERVAL: Duration = Duration::from_secs(3);
const LOGIC_LOAD_REFRESH_INTERVAL: Duration = Duration::from_secs(3);
const METRICS_REPORT_INTERVAL: Duration = Duration::from_secs(10);

#[derive(Debug, Error)]
pub enum ServiceError {
    #[error(transparent)]
    Config(#[from] xkk_config::ConfigError),
    #[error(transparent)]
    Frame(#[from] xframe::Error),
    #[error(transparent)]
    Log(#[from] xlog::Error),
    #[error("close Gate log worker: {0}")]
    LogClose(#[source] io::Error),
    #[error(transparent)]
    Mongo(#[from] xframe::xmongo::Error),
    #[error(transparent)]
    Persist(#[from] xkk_persist::Error),
    #[error(transparent)]
    Redis(#[from] xframe::xredis::Error),
    #[error(transparent)]
    Protocol(#[from] xkk_protocol::ProtocolError),
    #[error(transparent)]
    Rpc(#[from] xframe::xrpc::Error),
}

fn network_server(config: &Config) -> xframe::xnet::ServerConfig {
    let mut listeners = Vec::with_capacity(3);
    if let Some(port) = config.listeners.tcp_port {
        listeners.push(
            xframe::xnet::ListenEndpoint::tcp(format!("{}:{port}", config.node.listen_host))
                .external(),
        );
    }
    if let Some(port) = config.listeners.kcp_port {
        listeners.push(xframe::xnet::ListenEndpoint::kcp(format!(
            "{}:{port}",
            config.node.listen_host
        )));
    }
    if let Some(port) = config.listeners.websocket_port {
        listeners.push(xframe::xnet::ListenEndpoint::websocket(format!(
            "{}:{port}",
            config.node.listen_host
        )));
    }

    let transport = xframe::xnet::TransportOptions::default()
        .with_write_queue_capacity(SERVICE_WRITE_QUEUE_CAPACITY)
        .with_websocket_path(&config.listeners.websocket_path);
    xframe::xnet::ServerConfig::new(listeners)
        .with_role(xframe::xnet::ConnectionRole::client())
        .with_transport(transport)
        .with_admission(xframe::xnet::AdmissionConfig::new(
            MAX_EXTERNAL_HANDSHAKES,
            MAX_EXTERNAL_CONNECTIONS,
        ))
}

fn frame_config(config: &Config) -> Result<FrameConfig, ServiceError> {
    let primary_port = config
        .listeners
        .primary_port()
        .expect("validated Gate config has an enabled primary transport");
    let mut metadata = HashMap::from([
        ("protocol".to_string(), "client".to_string()),
        (
            "primary_transport".to_string(),
            config.listeners.primary_transport.as_str().to_string(),
        ),
    ]);
    if let Some(port) = config.listeners.tcp_port {
        metadata.insert("tcp_port".to_string(), port.to_string());
    }
    if let Some(port) = config.listeners.kcp_port {
        metadata.insert("kcp_port".to_string(), port.to_string());
    }
    if let Some(port) = config.listeners.websocket_port {
        metadata.insert("websocket_port".to_string(), port.to_string());
        metadata.insert(
            "websocket_path".to_string(),
            config.listeners.websocket_path.clone(),
        );
    }
    let node = NodeConfig::new(
        &config.node.cluster,
        ServiceType::Gate,
        config.node.instance_id,
        &config.node.advertise_host,
        primary_port,
    )?
    .with_versions(config.version.program, config.version.conf)
    .with_meta_data(metadata);
    let discovery = DiscoveryConfig::new(&config.infrastructure.etcd_dsn)?;
    let service_transport = xframe::xnet::TransportOptions::default()
        .with_write_queue_capacity(SERVICE_WRITE_QUEUE_CAPACITY);

    Ok(FrameConfig::new(node)
        .with_discovery(discovery)
        .with_service_client_transport(service_transport)
        .with_mongo(xframe::xmongo::Config::new(
            &config.infrastructure.mongo_dsn,
        )?)
        .with_redis(xframe::xredis::RedisConfig::new(
            &config.infrastructure.redis_dsn,
        )?)
        .with_rpc(RpcConfig::new(RPC_PENDING_CAPACITY)?)
        .with_shutdown(ShutdownConfig::new(SHUTDOWN_DRAIN_TIMEOUT)?))
}

fn gateway_settings(config: &Config) -> GatewaySettings {
    GatewaySettings {
        gate_id: config.node.instance_id,
        token_secret: config.security.token_secret.clone(),
        token_expire_seconds: config.security.token_expire_seconds,
    }
}

pub fn config_path() -> Result<PathBuf, ServiceError> {
    Ok(xkk_config::config_path("gate")?)
}

pub async fn run(config: Config) -> Result<(), ServiceError> {
    config.validate()?;
    let log_options = config.log.options("logs/gate.log");
    let frame_config = frame_config(&config)?;
    let network_server = network_server(&config);
    let gateway_settings = gateway_settings(&config);
    let cluster = config.node.cluster.clone();
    let instance_id = config.node.instance_id;
    let log_guard = xlog::init_global(log_options)?;

    let service: Result<(), ServiceError> = async {
        xkk_protocol::init_global_registry()?;
        let mut prepared = xframe::prepare(frame_config).await?;
        let handle = prepared.handle();
        let redis = handle
            .redis()
            .expect("Gate FrameConfig always enables Redis");
        let mongo = handle
            .mongo()
            .expect("Gate FrameConfig always enables Mongo");
        let _collections = xkk_persist::Collections::new(mongo)?;
        let application_redis = redis.clone();
        let online_count = Arc::new(AtomicI32::new(0));
        let sessions = ClientSessions::new(SessionConfig::HARD_LIMITS);
        let gateway = Gateway::new(
            handle,
            prepared.rpc().clone(),
            redis,
            sessions,
            online_count,
            gateway_settings,
        );
        gateway.register_rpc(prepared.rpc())?;
        prepared.add_server(network_server, gateway.clone());
        let frame = prepared
            .start(GateApplication::new(
                cluster,
                instance_id,
                application_redis,
                SERVICE_LOAD_PUBLISH_INTERVAL,
                LOGIC_LOAD_REFRESH_INTERVAL,
                METRICS_REPORT_INTERVAL,
                gateway,
            ))
            .await?;
        tracing::info!(instance_id, "Gate service started");
        let shutdown = frame.run_until_shutdown_signal().await;
        tracing::info!(
            instance_id,
            success = shutdown.is_ok(),
            "Gate service stopped"
        );
        shutdown?;
        Ok(())
    }
    .await;
    let log_close = log_guard.close().await;
    service?;
    log_close.map_err(ServiceError::LogClose)?;
    Ok(())
}

struct GateApplication {
    cluster: String,
    instance_id: i32,
    redis: xframe::xredis::Client,
    service_load_publish_interval: Duration,
    logic_refresh_interval: Duration,
    metrics_interval: Duration,
    gateway: Gateway,
    service_load_publish_task: Option<JoinHandle<()>>,
    logic_refresh_task: Option<JoinHandle<()>>,
    metrics_task: Option<JoinHandle<()>>,
}

impl GateApplication {
    fn new(
        cluster: String,
        instance_id: i32,
        redis: xframe::xredis::Client,
        service_load_publish_interval: Duration,
        logic_refresh_interval: Duration,
        metrics_interval: Duration,
        gateway: Gateway,
    ) -> Self {
        Self {
            cluster,
            instance_id,
            redis,
            service_load_publish_interval,
            logic_refresh_interval,
            metrics_interval,
            gateway,
            service_load_publish_task: None,
            logic_refresh_task: None,
            metrics_task: None,
        }
    }
}

impl Application for GateApplication {
    async fn start(&mut self, frame: FrameHandle) -> ApplicationResult {
        frame
            .watch_and_connect(self.cluster.clone(), ServiceType::Logic)
            .await?;
        frame
            .watch_and_connect(self.cluster.clone(), ServiceType::Public)
            .await?;
        publish_service_online(
            &self.redis,
            &self.cluster,
            ServiceType::Gate.as_i32(),
            self.instance_id,
            self.gateway.online_count(),
            service_online_ttl(self.service_load_publish_interval),
        )
        .await?;
        refresh_service_online(&frame, &self.redis, &self.cluster, ServiceType::Logic).await?;
        self.service_load_publish_task = Some(spawn_service_online_publish(
            self.redis.clone(),
            self.cluster.clone(),
            self.instance_id,
            self.gateway.clone(),
            self.service_load_publish_interval,
        ));
        self.logic_refresh_task = Some(spawn_logic_online_refresh(
            frame.clone(),
            self.redis.clone(),
            self.cluster.clone(),
            self.logic_refresh_interval,
        ));
        self.metrics_task = spawn_metrics(frame, self.gateway.clone(), self.metrics_interval);
        Ok(())
    }

    async fn shutdown(&mut self, _frame: FrameHandle) -> ApplicationResult {
        stop_task(
            &mut self.service_load_publish_task,
            "Gate service load publish",
        )
        .await;
        stop_task(&mut self.logic_refresh_task, "Gate Logic load refresh").await;
        stop_task(&mut self.metrics_task, "Gate metrics").await;
        if let Err(error) = delete_service_online(
            &self.redis,
            &self.cluster,
            ServiceType::Gate.as_i32(),
            self.instance_id,
        )
        .await
        {
            tracing::warn!(%error, "Gate service online cleanup failed");
        }
        self.gateway.shutdown().await;
        Ok(())
    }
}

async fn refresh_service_online(
    frame: &FrameHandle,
    redis: &xframe::xredis::Client,
    cluster: &str,
    service_type: ServiceType,
) -> ApplicationResult {
    let instance_ids = frame.service_instance_ids(service_type)?;
    let counts =
        load_service_online_counts(redis, cluster, service_type.as_i32(), instance_ids).await?;
    frame.update_online_counts(
        service_type,
        counts
            .into_iter()
            .map(|count| (count.instance_id, count.online_count)),
    )?;
    Ok(())
}

fn spawn_service_online_publish(
    redis: xframe::xredis::Client,
    cluster: String,
    instance_id: i32,
    gateway: Gateway,
    interval: Duration,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let ttl = service_online_ttl(interval);
        let mut ticker = tokio::time::interval(interval);
        ticker.tick().await;
        loop {
            ticker.tick().await;
            let online_count = gateway.online_count();
            if let Err(error) = publish_service_online(
                &redis,
                &cluster,
                ServiceType::Gate.as_i32(),
                instance_id,
                online_count,
                ttl,
            )
            .await
            {
                tracing::warn!(online_count, %error, "Gate service online publish failed");
            }
        }
    })
}

fn spawn_logic_online_refresh(
    frame: FrameHandle,
    redis: xframe::xredis::Client,
    cluster: String,
    interval: Duration,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(interval);
        ticker.tick().await;
        loop {
            ticker.tick().await;
            if let Err(error) =
                refresh_service_online(&frame, &redis, &cluster, ServiceType::Logic).await
            {
                tracing::warn!(%error, "Gate Logic online refresh failed");
            }
        }
    })
}

async fn stop_task(task: &mut Option<JoinHandle<()>>, name: &'static str) {
    let Some(task) = task.take() else {
        return;
    };
    task.abort();
    if let Err(error) = task.await
        && !error.is_cancelled()
    {
        tracing::error!(task = name, %error, "Gate background task failed");
    }
}

fn spawn_metrics(
    frame: FrameHandle,
    gateway: Gateway,
    interval: Duration,
) -> Option<JoinHandle<()>> {
    if interval.is_zero() {
        return None;
    }
    Some(tokio::spawn(async move {
        let mut ticker = tokio::time::interval(interval);
        let mut last_rpc_pending_rejected = 0;
        let mut last_write_queue_rejected = 0;
        let mut last_handshakes_rejected = 0;
        let mut last_connections_rejected = 0;
        ticker.tick().await;
        loop {
            ticker.tick().await;
            let expired = gateway.sessions().prune_expired(Instant::now());
            let sessions = gateway.sessions().stats();
            let online_count = gateway.online_count();
            let login = gateway.login_stats();
            let stats = frame.stats();
            let write_queue_rejected = stats.sessions.outbound_rejected_full
                + stats
                    .listeners
                    .iter()
                    .map(|listener| listener.outbound_rejected_full)
                    .sum::<u64>();
            let handshakes_rejected = stats
                .listeners
                .iter()
                .map(|listener| listener.rejected_external_handshakes)
                .sum::<u64>();
            let connections_rejected = stats
                .listeners
                .iter()
                .map(|listener| listener.rejected_external_connections)
                .sum::<u64>();
            if stats.rpc.pending_rejected > last_rpc_pending_rejected {
                tracing::error!(
                    rejected = stats.rpc.pending_rejected - last_rpc_pending_rejected,
                    total_rejected = stats.rpc.pending_rejected,
                    limit = RPC_PENDING_CAPACITY,
                    "Gate RPC pending hard limit exceeded"
                );
                last_rpc_pending_rejected = stats.rpc.pending_rejected;
            }
            if write_queue_rejected > last_write_queue_rejected {
                tracing::error!(
                    rejected = write_queue_rejected - last_write_queue_rejected,
                    total_rejected = write_queue_rejected,
                    limit = SERVICE_WRITE_QUEUE_CAPACITY,
                    "Gate write queue hard limit exceeded"
                );
                last_write_queue_rejected = write_queue_rejected;
            }
            if handshakes_rejected > last_handshakes_rejected {
                tracing::error!(
                    rejected = handshakes_rejected - last_handshakes_rejected,
                    total_rejected = handshakes_rejected,
                    limit = MAX_EXTERNAL_HANDSHAKES,
                    "Gate external handshake hard limit exceeded"
                );
                last_handshakes_rejected = handshakes_rejected;
            }
            if connections_rejected > last_connections_rejected {
                tracing::error!(
                    rejected = connections_rejected - last_connections_rejected,
                    total_rejected = connections_rejected,
                    limit = MAX_EXTERNAL_CONNECTIONS,
                    "Gate external connection hard limit exceeded"
                );
                last_connections_rejected = connections_rejected;
            }
            tracing::info!(
                frame_state = ?stats.state,
                active_sessions = stats.sessions.active_sessions,
                online_players = online_count,
                resume_players = sessions.offline,
                outbox_messages = sessions.outbox_messages,
                expired_resume_players = expired,
                write_queue_depth = stats.sessions.outbound_queue_depth,
                rpc_pending = stats.rpc.pending,
                rpc_inbound_active = stats.rpc.inbound_active,
                rpc_pending_rejected = stats.rpc.pending_rejected,
                login_count = login.total.count(),
                login_avg_us = login.total.average_micros(),
                login_p99_us = login.total.percentile_micros(99.0),
                login_max_us = login.total.max_micros,
                login_token_avg_us = login.token_decode.average_micros(),
                login_redis_load_avg_us = login.redis_load_online.average_micros(),
                login_redis_load_p99_us = login.redis_load_online.percentile_micros(99.0),
                login_route_avg_us = login.route_select.average_micros(),
                login_logic_rpc_avg_us = login.logic_rpc.average_micros(),
                login_logic_rpc_p99_us = login.logic_rpc.percentile_micros(99.0),
                login_bind_avg_us = login.session_bind.average_micros(),
                login_redis_save_avg_us = login.redis_save_online.average_micros(),
                login_redis_save_p99_us = login.redis_save_online.percentile_micros(99.0),
                login_send_avg_us = login.response_send.average_micros(),
                listeners = ?stats.listeners,
                "Gate runtime stats"
            );
        }
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn example_config_enables_all_external_transports() {
        let config = Config::parse(
            include_str!("../../../config/common.yaml"),
            include_str!("../../../config/gate.yaml"),
            include_str!("../../../config/version.json"),
        )
        .unwrap();
        let frame = frame_config(&config).unwrap();
        let server = network_server(&config);

        assert_eq!(server.listeners.len(), 3);
        assert!(frame.service_server.is_none());
        assert_eq!(frame.node.port(), 3201);
        assert_eq!(frame.rpc.pending_capacity(), RPC_PENDING_CAPACITY);
        assert_eq!(
            frame.service_client_transport.write_queue_capacity,
            SERVICE_WRITE_QUEUE_CAPACITY
        );
        assert_eq!(
            frame.node.meta_data().get("primary_transport").unwrap(),
            "tcp"
        );
    }
}
