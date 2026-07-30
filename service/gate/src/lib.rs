mod gateway;
mod session;

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
    delete_service_online, publish_service_online, refresh_service_online, service_online_ttl,
};

use crate::{
    gateway::{Gateway, GatewaySettings},
    session::{ClientSessions, SessionConfig},
};
pub use xkk_config::GateConfig as Config;

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
        .with_write_queue_capacity(config.capacity.write_queue)
        .with_websocket_path(&config.listeners.websocket_path);
    xframe::xnet::ServerConfig::new(listeners)
        .with_role(xframe::xnet::ConnectionRole::client())
        .with_transport(transport)
        .with_admission(xframe::xnet::AdmissionConfig::new(
            config.capacity.max_external_handshakes,
            config.capacity.max_external_connections,
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
    .with_versions(config.node.pro_version, config.node.conf_version)
    .with_meta_data(metadata);
    let discovery = DiscoveryConfig::new(&config.infrastructure.etcd_dsn)?.with_lease_ttl(
        Duration::from_secs(config.infrastructure.etcd_lease_ttl_seconds),
    )?;
    let service_transport = xframe::xnet::TransportOptions::default()
        .with_write_queue_capacity(config.capacity.write_queue);

    Ok(FrameConfig::new(node)
        .with_discovery(discovery)
        .with_service_client_transport(service_transport)
        .with_mongo(xframe::xmongo::Config::new(
            &config.infrastructure.mongo_dsn,
        )?)
        .with_redis(xframe::xredis::RedisConfig::new(
            &config.infrastructure.redis_dsn,
        )?)
        .with_rpc(RpcConfig::new(config.capacity.rpc_pending)?)
        .with_shutdown(ShutdownConfig::new(Duration::from_secs(
            config.runtime.shutdown_drain_seconds,
        ))?))
}

fn session_config(config: &Config) -> SessionConfig {
    SessionConfig {
        outbox_messages: config.capacity.outbox_messages,
        resume_ttl: Duration::from_secs(config.runtime.resume_seconds),
        reconnect_total: config.runtime.reconnect_total,
        reconnect_window: Duration::from_secs(config.runtime.reconnect_window_seconds),
        reconnect_window_count: config.runtime.reconnect_window_count,
        request_window: Duration::from_millis(config.runtime.client_request_window_ms),
        request_window_count: config.capacity.client_request_window_count,
        burst_window: Duration::from_millis(config.runtime.client_burst_window_ms),
        burst_count: config.capacity.client_burst_count,
    }
}

fn gateway_settings(config: &Config) -> GatewaySettings {
    GatewaySettings {
        gate_id: config.node.instance_id,
        client_mailbox: config.capacity.client_mailbox,
        rpc_timeout: Duration::from_millis(config.runtime.rpc_timeout_ms),
        token_secret: config.security.token_secret.clone(),
        token_expire_seconds: config.security.token_expire_seconds,
        shutdown_cleanup_concurrency: config.capacity.shutdown_cleanup_concurrency,
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
    let session_config = session_config(&config);
    let gateway_settings = gateway_settings(&config);
    let cluster = config.node.cluster.clone();
    let service_load_interval = Duration::from_secs(config.runtime.service_load_interval_seconds);
    let metrics_interval = Duration::from_secs(config.runtime.metrics_interval_seconds);
    let instance_id = config.node.instance_id;
    let log_guard = xlog::init_global(log_options)?;

    let service: Result<(), ServiceError> = async {
        let mut prepared = xframe::prepare(frame_config).await?;
        prepared.register_client_protocol(xkk_protocol::client_registry()?);
        let handle = prepared.handle();
        let redis = handle
            .redis()
            .expect("Gate FrameConfig always enables Redis");
        let application_redis = redis.clone();
        let online_count = Arc::new(AtomicI32::new(0));
        let sessions = ClientSessions::new(session_config);
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
                service_load_interval,
                metrics_interval,
                gateway,
            ))
            .await?;
        xlog::info!(instance_id, "Gate service started");
        let shutdown = frame.run_until_shutdown_signal().await;
        xlog::info!(
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
    service_load_interval: Duration,
    metrics_interval: Duration,
    gateway: Gateway,
    service_load_task: Option<JoinHandle<()>>,
    metrics_task: Option<JoinHandle<()>>,
}

impl GateApplication {
    fn new(
        cluster: String,
        instance_id: i32,
        redis: xframe::xredis::Client,
        service_load_interval: Duration,
        metrics_interval: Duration,
        gateway: Gateway,
    ) -> Self {
        Self {
            cluster,
            instance_id,
            redis,
            service_load_interval,
            metrics_interval,
            gateway,
            service_load_task: None,
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
            ServiceType::Gate,
            self.instance_id,
            self.gateway.online_count(),
            service_online_ttl(self.service_load_interval),
        )
        .await?;
        refresh_service_online(&frame, &self.redis, &self.cluster, ServiceType::Logic).await?;
        self.service_load_task = Some(spawn_service_loads(
            frame.clone(),
            self.redis.clone(),
            self.cluster.clone(),
            self.instance_id,
            self.gateway.clone(),
            self.service_load_interval,
        ));
        self.metrics_task = spawn_metrics(frame, self.gateway.clone(), self.metrics_interval);
        Ok(())
    }

    async fn shutdown(&mut self, _frame: FrameHandle) -> ApplicationResult {
        if let Some(task) = self.service_load_task.take() {
            task.abort();
            let _ = task.await;
        }
        if let Some(task) = self.metrics_task.take() {
            task.abort();
            let _ = task.await;
        }
        if let Err(error) = delete_service_online(
            &self.redis,
            &self.cluster,
            ServiceType::Gate,
            self.instance_id,
        )
        .await
        {
            xlog::warn!(%error, "Gate service online cleanup failed");
        }
        self.gateway.shutdown().await;
        Ok(())
    }
}

fn spawn_service_loads(
    frame: FrameHandle,
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
                ServiceType::Gate,
                instance_id,
                online_count,
                ttl,
            )
            .await
            {
                xlog::warn!(online_count, %error, "Gate service online publish failed");
            }
            if let Err(error) =
                refresh_service_online(&frame, &redis, &cluster, ServiceType::Logic).await
            {
                xlog::warn!(%error, "Gate Logic online refresh failed");
            }
        }
    })
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
        ticker.tick().await;
        loop {
            ticker.tick().await;
            let expired = gateway.sessions().prune_expired(Instant::now());
            let sessions = gateway.sessions().stats();
            let online_count = gateway.online_count();
            let stats = frame.stats();
            xlog::info!(
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
        let config = Config::parse(include_str!("../../../config/gate.yaml")).unwrap();
        let frame = frame_config(&config).unwrap();
        let server = network_server(&config);

        assert_eq!(server.listeners.len(), 3);
        assert!(frame.service_server.is_none());
        assert_eq!(frame.node.port(), 3201);
        assert_eq!(config.runtime.service_load_interval_seconds, 3);
        assert_eq!(frame.service_client_transport.write_queue_capacity, 1024);
        assert_eq!(
            frame.node.meta_data().get("primary_transport").unwrap(),
            "tcp"
        );
    }
}
