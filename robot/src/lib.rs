use std::{
    error::Error,
    fmt,
    future::Future,
    net::SocketAddr,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use hyper::{Request, StatusCode, Uri, header::CONTENT_TYPE};
use hyper_util::{
    client::legacy::{Client as HttpClient, connect::HttpConnector},
    rt::TokioExecutor,
};
use prost::Message;
use serde::{Serialize, de::DeserializeOwned};
use tokio::{
    sync::mpsc,
    time::{Instant, sleep, timeout},
};
use xkk_config::{RobotConfig, RobotTransport};
use xkk_protocol::{MsgId, code, pb};
use xnet::{
    Client as NetClient, ClientConfig, ConnectEndpoint, Connection, Frame, Handler, SessionId,
    SessionManager, TransportOptions,
};
use xproto::cs::{CsHead, CsPacket};

const LOGIN_PATH: &str = "/v1/auth/login";
const USE_ROLE_PATH: &str = "/v1/auth/use-role";
const EVENT_CAPACITY: usize = 8;
const LOGIN_SEQUENCE: u32 = 1;

type JsonClient = HttpClient<HttpConnector, Full<Bytes>>;
type Result<T> = std::result::Result<T, RobotError>;

#[derive(Debug)]
pub struct RobotError(String);

impl fmt::Display for RobotError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl Error for RobotError {}

#[derive(Debug, PartialEq, Eq)]
pub struct LoginResult {
    pub gid: i64,
    pub session_id: i64,
    pub logic_id: i32,
    pub public_id: i32,
    pub transport: RobotTransport,
    pub host: String,
    pub port: u16,
}

#[derive(Debug)]
enum Event {
    Connected(Connection),
    Packet(Frame),
    Disconnected(SessionId),
}

#[derive(Clone)]
struct EventHandler {
    events: mpsc::Sender<Event>,
}

impl EventHandler {
    fn emit(&self, event: Event) {
        self.events
            .try_send(event)
            .expect("xkk-robot event queue exhausted");
    }
}

impl Handler for EventHandler {
    fn on_connected(&self, conn: Connection) {
        self.emit(Event::Connected(conn));
    }

    fn on_packet(&self, frame: Frame) {
        self.emit(Event::Packet(frame));
    }

    fn on_disconnected(&self, conn: Connection) {
        self.emit(Event::Disconnected(conn.session_id()));
    }
}

pub async fn login(config: &RobotConfig) -> Result<LoginResult> {
    let deadline = Instant::now() + Duration::from_secs(config.timeout_seconds);
    let http = HttpClient::builder(TokioExecutor::new()).build(HttpConnector::new());
    let auth = post_json::<_, pb::AuthLoginRsp>(
        &http,
        &config.auth_url,
        LOGIN_PATH,
        &pb::AuthLoginReq {
            account: config.account.clone(),
            credential: config.credential.clone(),
            device: Some(pb::DeviceInfo {
                device_id: config.device_id.clone(),
                platform: config.platform.clone(),
                client_version: config.client_version.clone(),
            }),
        },
        deadline,
        "Auth login",
    )
    .await?;
    let (gid, token) = validate_auth_login(&config.account, auth)?;

    let endpoints = admit_role(&http, config, gid, &token, deadline).await?;
    let endpoint = select_endpoint(&endpoints, config.transport)?;
    let addr = resolve_endpoint(&endpoint, deadline).await?;
    let gate = connect_gate(&endpoint, addr, deadline)?;
    login_gate(gate, config, gid, token, endpoint, deadline).await
}

async fn admit_role(
    http: &JsonClient,
    config: &RobotConfig,
    gid: i64,
    token: &str,
    deadline: Instant,
) -> Result<Vec<pb::Endpoint>> {
    let request = pb::AuthUseRoleReq {
        gid,
        token: token.to_string(),
        device_id: config.device_id.clone(),
    };

    loop {
        let response = post_json::<_, pb::AuthUseRoleRsp>(
            http,
            &config.auth_url,
            USE_ROLE_PATH,
            &request,
            deadline,
            "Auth use-role",
        )
        .await?;
        require_ok(response.status.as_ref(), "Auth use-role")?;
        if response.gid != gid {
            return Err(failure(format!(
                "Auth use-role returned gid {}, expected {gid}",
                response.gid
            )));
        }

        let Some(queue) = response.queue else {
            if response.endpoints.is_empty() {
                return Err(failure("Auth use-role returned no Gate endpoints"));
            }
            return Ok(response.endpoints);
        };
        if !response.endpoints.is_empty() {
            return Err(failure(
                "Auth use-role returned both queue state and Gate endpoints",
            ));
        }
        wait_for_queue(&queue, deadline).await?;
    }
}

async fn wait_for_queue(queue: &pb::LoginQueue, deadline: Instant) -> Result<()> {
    if queue.position <= 0 || queue.next_request_time <= 0 {
        return Err(failure("Auth use-role returned invalid queue state"));
    }
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| failure(format!("read system clock: {error}")))?
        .as_secs();
    let next = u64::try_from(queue.next_request_time)
        .map_err(|_| failure("Auth use-role returned invalid next_request_time"))?;
    let wait = Duration::from_secs(next.saturating_sub(now));
    let remaining = remaining(deadline, "Auth use-role queue")?;
    if wait >= remaining {
        return Err(failure("Auth use-role queue timed out"));
    }
    sleep(wait).await;
    Ok(())
}

async fn post_json<T, R>(
    client: &JsonClient,
    base_url: &str,
    path: &str,
    value: &T,
    deadline: Instant,
    operation: &str,
) -> Result<R>
where
    T: Serialize,
    R: DeserializeOwned,
{
    let url = format!("{}{}", base_url.trim_end_matches('/'), path);
    let uri: Uri = url
        .parse()
        .map_err(|error| failure(format!("invalid {operation} URL {url}: {error}")))?;
    let body = serde_json::to_vec(value)
        .map_err(|error| failure(format!("encode {operation} request: {error}")))?;
    let request = Request::post(uri)
        .header(CONTENT_TYPE, "application/json")
        .body(Full::new(Bytes::from(body)))
        .map_err(|error| failure(format!("build {operation} request: {error}")))?;

    let response = before_deadline(deadline, operation, client.request(request))
        .await?
        .map_err(|error| failure(format!("send {operation} request: {error}")))?;
    let status = response.status();
    let body = before_deadline(deadline, operation, response.into_body().collect())
        .await?
        .map_err(|error| failure(format!("read {operation} response: {error}")))?
        .to_bytes();
    if status != StatusCode::OK {
        return Err(failure(format!(
            "{operation} returned HTTP {status}: {}",
            String::from_utf8_lossy(&body)
        )));
    }
    serde_json::from_slice(&body)
        .map_err(|error| failure(format!("decode {operation} response: {error}")))
}

fn validate_auth_login(account: &str, response: pb::AuthLoginRsp) -> Result<(i64, String)> {
    require_ok(response.status.as_ref(), "Auth login")?;
    if response.account != account {
        return Err(failure(format!(
            "Auth login returned account {}, expected {account}",
            response.account
        )));
    }
    let role = response
        .roles
        .first()
        .ok_or_else(|| failure("Auth login returned no role"))?;
    if role.gid <= 0 {
        return Err(failure("Auth login returned invalid gid"));
    }
    if response.token.is_empty() {
        return Err(failure("Auth login returned an empty token"));
    }
    Ok((role.gid, response.token))
}

fn select_endpoint(endpoints: &[pb::Endpoint], transport: RobotTransport) -> Result<pb::Endpoint> {
    let endpoint = endpoints
        .iter()
        .find(|endpoint| endpoint.transport == transport.as_str())
        .ok_or_else(|| {
            failure(format!(
                "Auth use-role returned no {} endpoint",
                transport.as_str()
            ))
        })?;
    if endpoint.host.is_empty() || endpoint.port == 0 || endpoint.port > u16::MAX.into() {
        return Err(failure(format!(
            "Auth use-role returned invalid {} endpoint",
            transport.as_str()
        )));
    }
    if transport == RobotTransport::Websocket && !endpoint.path.starts_with('/') {
        return Err(failure(
            "Auth use-role returned invalid WebSocket endpoint path",
        ));
    }
    Ok(endpoint.clone())
}

async fn resolve_endpoint(endpoint: &pb::Endpoint, deadline: Instant) -> Result<SocketAddr> {
    let port = u16::try_from(endpoint.port).expect("endpoint port validated");
    let addresses = before_deadline(
        deadline,
        "Gate endpoint resolution",
        tokio::net::lookup_host((endpoint.host.as_str(), port)),
    )
    .await?
    .map_err(|error| {
        failure(format!(
            "resolve Gate endpoint {}:{port}: {error}",
            endpoint.host
        ))
    })?;
    addresses.into_iter().next().ok_or_else(|| {
        failure(format!(
            "Gate endpoint {}:{port} resolved no address",
            endpoint.host
        ))
    })
}

fn connect_gate(
    endpoint: &pb::Endpoint,
    addr: SocketAddr,
    deadline: Instant,
) -> Result<GateClient> {
    remaining(deadline, "Gate connection")?;
    let connect_endpoint = match endpoint.transport.as_str() {
        "tcp" => ConnectEndpoint::tcp(addr),
        "kcp" => ConnectEndpoint::kcp(addr),
        "websocket" => ConnectEndpoint::websocket(addr),
        _ => return Err(failure("unsupported Gate transport")),
    };
    let mut client_config = ClientConfig::new(connect_endpoint);
    if endpoint.transport == "websocket" {
        client_config = client_config
            .with_transport(TransportOptions::default().with_websocket_path(endpoint.path.clone()));
    }
    let (events, receiver) = mpsc::channel(EVENT_CAPACITY);
    let client = NetClient::connect(
        client_config,
        EventHandler { events },
        SessionManager::new(),
        |_| true,
        |_| {},
    )
    .map_err(|error| failure(format!("start Gate connection: {error}")))?;
    Ok(GateClient { client, receiver })
}

struct GateClient {
    client: NetClient,
    receiver: mpsc::Receiver<Event>,
}

async fn login_gate(
    mut gate: GateClient,
    config: &RobotConfig,
    gid: i64,
    token: String,
    endpoint: pb::Endpoint,
    deadline: Instant,
) -> Result<LoginResult> {
    let result = async {
        let connection = receive_connection(&mut gate.receiver, deadline).await?;
        send(
            &connection,
            MsgId::LoginReq,
            LOGIN_SEQUENCE,
            &pb::LoginReq {
                gid,
                token,
                device_id: config.device_id.clone(),
            },
        )?;
        let (head, response): (_, pb::LoginRsp) =
            receive_packet(&mut gate.receiver, MsgId::LoginRsp, deadline).await?;
        validate_login_rsp(gid, head, &response)?;
        Ok(LoginResult {
            gid,
            session_id: response.session_id,
            logic_id: response.logic_id,
            public_id: response.public_id,
            transport: config.transport,
            host: endpoint.host,
            port: u16::try_from(endpoint.port).expect("endpoint port validated"),
        })
    }
    .await;
    gate.client.shutdown().await;
    result
}

async fn receive_connection(
    events: &mut mpsc::Receiver<Event>,
    deadline: Instant,
) -> Result<Connection> {
    match before_deadline(deadline, "Gate connection", events.recv()).await? {
        Some(Event::Connected(connection)) => Ok(connection),
        Some(Event::Packet(_)) => Err(failure("received Gate packet before connection event")),
        Some(Event::Disconnected(session_id)) => Err(failure(format!(
            "Gate disconnected session {session_id} while connecting"
        ))),
        None => Err(failure("Gate event stream closed while connecting")),
    }
}

fn send<M>(connection: &Connection, msgid: MsgId, seq: u32, body: &M) -> Result<()>
where
    M: Message,
{
    let payload = CsPacket::encode_message(
        CsHead {
            msgid: msgid.as_u16(),
            seq,
            ..Default::default()
        },
        body,
    )
    .map_err(|error| failure(format!("encode {msgid:?}: {error}")))?;
    if !connection.send_flush(payload) {
        return Err(failure(format!("Gate send rejected for {msgid:?}")));
    }
    Ok(())
}

async fn receive_packet<M>(
    events: &mut mpsc::Receiver<Event>,
    expected: MsgId,
    deadline: Instant,
) -> Result<(CsHead, M)>
where
    M: Message + Default,
{
    let event = before_deadline(deadline, "Gate login response", events.recv()).await?;
    let frame = match event {
        Some(Event::Packet(frame)) => frame,
        Some(Event::Disconnected(session_id)) => {
            return Err(failure(format!(
                "Gate disconnected session {session_id} before {expected:?}"
            )));
        }
        Some(Event::Connected(connection)) => {
            return Err(failure(format!(
                "unexpected Gate connection {} before {expected:?}",
                connection.session_id()
            )));
        }
        None => return Err(failure("Gate event stream closed during login")),
    };
    let packet = CsPacket::decode(&frame.payload)
        .map_err(|error| failure(format!("decode Gate packet: {error}")))?;
    if packet.head.msgid != expected.as_u16() {
        return Err(failure(format!(
            "expected {expected:?}, got message {}",
            packet.head.msgid
        )));
    }
    let body =
        M::decode(packet.body).map_err(|error| failure(format!("decode {expected:?}: {error}")))?;
    Ok((packet.head, body))
}

fn validate_login_rsp(gid: i64, head: CsHead, response: &pb::LoginRsp) -> Result<()> {
    require_ok(response.status.as_ref(), "Gate login")?;
    if head.ack != LOGIN_SEQUENCE || head.seq == 0 {
        return Err(failure(format!(
            "Gate login returned invalid sequence: seq={} ack={}",
            head.seq, head.ack
        )));
    }
    if response.gid != gid
        || response.session_id <= 0
        || response.logic_id <= 0
        || response.public_id <= 0
        || response.player.as_ref().map(|player| player.gid) != Some(gid)
    {
        return Err(failure("Gate login returned invalid player state"));
    }
    Ok(())
}

fn require_ok(status: Option<&pb::Status>, operation: &str) -> Result<()> {
    match status {
        Some(status) if status.code == code::OK => Ok(()),
        Some(status) => Err(failure(format!(
            "{operation} failed: code={} message={}",
            status.code, status.message
        ))),
        None => Err(failure(format!("{operation} returned no status"))),
    }
}

async fn before_deadline<T>(
    deadline: Instant,
    operation: &str,
    future: impl Future<Output = T>,
) -> Result<T> {
    timeout(remaining(deadline, operation)?, future)
        .await
        .map_err(|_| failure(format!("{operation} timed out")))
}

fn remaining(deadline: Instant, operation: &str) -> Result<Duration> {
    deadline
        .checked_duration_since(Instant::now())
        .filter(|duration| !duration.is_zero())
        .ok_or_else(|| failure(format!("{operation} timed out")))
}

fn failure(message: impl Into<String>) -> RobotError {
    RobotError(message.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selects_requested_endpoint_and_keeps_websocket_path() {
        let endpoints = vec![
            pb::Endpoint {
                transport: "tcp".to_string(),
                host: "127.0.0.1".to_string(),
                port: 3201,
                path: String::new(),
            },
            pb::Endpoint {
                transport: "websocket".to_string(),
                host: "127.0.0.1".to_string(),
                port: 3203,
                path: "/ws".to_string(),
            },
        ];

        let selected = select_endpoint(&endpoints, RobotTransport::Websocket).unwrap();

        assert_eq!(selected.port, 3203);
        assert_eq!(selected.path, "/ws");
    }

    #[test]
    fn login_response_requires_authoritative_player_state() {
        let response = pb::LoginRsp {
            status: Some(pb::Status {
                code: code::OK,
                message: String::new(),
            }),
            gid: 1001,
            logic_id: 1,
            public_id: 1,
            player: Some(pb::PlayerInfo {
                gid: 1001,
                ..Default::default()
            }),
            session_id: 9,
            ..Default::default()
        };
        let head = CsHead {
            seq: 2,
            ack: LOGIN_SEQUENCE,
            ..Default::default()
        };

        validate_login_rsp(1001, head, &response).unwrap();
        assert!(validate_login_rsp(1002, head, &response).is_err());
    }
}
