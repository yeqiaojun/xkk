use std::net::SocketAddr;

use prost::Message;
use tokio::{sync::mpsc, time::Instant};
use xkk_config::{RobotConfig, RobotTransport};
use xkk_protocol::{MsgId, pb};
use xnet::{
    Client as NetClient, ClientConfig, ConnectEndpoint, Connection, Frame, Handler, SessionId,
    SessionManager, TransportOptions,
};
use xproto::cs::{CsHead, CsPacket};

use crate::{
    client::{LoginResult, before_deadline, remaining, require_ok},
    error::{Result, failure},
};

const EVENT_CAPACITY: usize = 8;
const LOGIN_SEQUENCE: u32 = 1;

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

pub(crate) fn select_endpoint(
    endpoints: &[pb::Endpoint],
    transport: RobotTransport,
) -> Result<pb::Endpoint> {
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

pub(crate) async fn resolve_endpoint(
    endpoint: &pb::Endpoint,
    deadline: Instant,
) -> Result<SocketAddr> {
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

pub(crate) fn connect_gate(
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
    }
    .external();
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

pub(crate) struct GateClient {
    client: NetClient,
    receiver: mpsc::Receiver<Event>,
}

pub(crate) async fn login_gate(
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
    connection
        .send(payload)
        .map_err(|error| failure(format!("Gate send rejected for {msgid:?}: {error}")))?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use xkk_protocol::code;

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
