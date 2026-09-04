use std::{env, error::Error, fmt, net::SocketAddr, time::Duration};

use prost::Message;
use tokio::{sync::mpsc, time::timeout};
use xkk_protocol::{MsgId, code, pb};
use xnet::{Client, ClientConfig, ConnectEndpoint, Connection, Frame, Handler, SessionId, SessionManager};
use xproto::cs::{CsHead, CsPacket};

const EVENT_TIMEOUT: Duration = Duration::from_secs(5);
const EVENT_CAPACITY: usize = 16;

type Result<T> = std::result::Result<T, Box<dyn Error + Send + Sync>>;

#[derive(Debug)]
struct SmokeError(String);

impl fmt::Display for SmokeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl Error for SmokeError {}

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
        self.events.try_send(event).expect("cluster smoke event queue exhausted");
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

#[tokio::main]
async fn main() -> Result<()> {
    let (addr, gid, token, device_id) = arguments()?;
    run(addr, gid, token, device_id).await
}

fn arguments() -> Result<(SocketAddr, i64, String, String)> {
    let mut args = env::args().skip(1);
    let usage = "usage: xkk-cluster-smoke <gate-addr> <gid> <token> <device-id>";
    let addr = args.next().ok_or_else(|| SmokeError(usage.to_string()))?.parse()?;
    let gid = args.next().ok_or_else(|| SmokeError(usage.to_string()))?.parse()?;
    let token = args.next().ok_or_else(|| SmokeError(usage.to_string()))?;
    let device_id = args.next().ok_or_else(|| SmokeError(usage.to_string()))?;
    if args.next().is_some() || gid <= 0 || token.is_empty() || device_id.len() < 8 {
        return Err(SmokeError(usage.to_string()).into());
    }
    Ok((addr, gid, token, device_id))
}

async fn run(addr: SocketAddr, gid: i64, token: String, device_id: String) -> Result<()> {
    let (first_client, first_conn, mut first_events) = connect(addr).await?;
    send(&first_conn, MsgId::LoginReq, 1, 0, &pb::LoginReq { gid, token: token.clone(), device_id: device_id.clone() })?;
    let (login_head, login): (_, pb::LoginRsp) = receive(&mut first_events, MsgId::LoginRsp).await?;
    require_ok(login.status.as_ref(), "Gate login")?;
    if login.gid != gid || login.session_id <= 0 || login.player.as_ref().map(|p| p.gid) != Some(gid) {
        return Err(SmokeError("Gate login returned invalid player state".to_string()).into());
    }
    require_ack(login_head, 1, "Gate login")?;
    let previous_session = login.session_id;
    first_client.shutdown().await;

    let (second_client, second_conn, mut second_events) = connect(addr).await?;
    send(
        &second_conn,
        MsgId::ReconnectReq,
        2,
        login_head.seq,
        &pb::ReconnectReq { gid, token, device_id, previous_session, ack: login_head.seq },
    )?;
    let (reconnect_head, reconnect): (_, pb::ReconnectRsp) = receive(&mut second_events, MsgId::ReconnectRsp).await?;
    require_ok(reconnect.status.as_ref(), "Gate reconnect")?;
    require_ack(reconnect_head, 2, "Gate reconnect")?;
    if reconnect.session_id <= 0 || reconnect.session_id == previous_session {
        return Err(SmokeError("Gate reconnect did not publish a new session".to_string()).into());
    }

    send(&second_conn, MsgId::PlayerInfoReq, 3, reconnect_head.seq, &pb::PlayerInfoReq {})?;
    let (player_head, player): (_, pb::PlayerInfoRsp) = receive(&mut second_events, MsgId::PlayerInfoRsp).await?;
    require_ok(player.status.as_ref(), "Logic player info")?;
    require_ack(player_head, 3, "Logic player info")?;
    if player.player.as_ref().map(|value| value.gid) != Some(gid) {
        return Err(SmokeError("Logic returned the wrong player".to_string()).into());
    }

    send(&second_conn, MsgId::MailListReq, 4, player_head.seq, &pb::MailListReq {})?;
    let (mail_head, mail): (_, pb::MailListRsp) = receive(&mut second_events, MsgId::MailListRsp).await?;
    require_ok(mail.status.as_ref(), "Public mail list")?;
    require_ack(mail_head, 4, "Public mail list")?;

    send(&second_conn, MsgId::LogoutReq, 5, mail_head.seq, &pb::LogoutReq {})?;
    let (logout_head, logout): (_, pb::LogoutRsp) = receive(&mut second_events, MsgId::LogoutRsp).await?;
    require_ok(logout.status.as_ref(), "Gate logout")?;
    require_ack(logout_head, 5, "Gate logout")?;
    second_client.shutdown().await;

    println!(
        "Gate smoke passed: gid={gid} first_session={previous_session} resumed_session={} mails={}",
        reconnect.session_id,
        mail.mails.len()
    );
    Ok(())
}

async fn connect(addr: SocketAddr) -> Result<(Client, Connection, mpsc::Receiver<Event>)> {
    let (events, mut receiver) = mpsc::channel(EVENT_CAPACITY);
    let client = Client::connect(
        ClientConfig::new(ConnectEndpoint::tcp(addr).external()),
        EventHandler { events },
        SessionManager::new(),
        |_| true,
        |_| {},
    )?;
    let conn = match timeout(EVENT_TIMEOUT, receiver.recv()).await {
        Ok(Some(Event::Connected(conn))) => conn,
        Ok(Some(event)) => {
            return Err(SmokeError(format!("expected Gate connection, got {event:?}")).into());
        }
        Ok(None) => return Err(SmokeError("Gate event stream closed".to_string()).into()),
        Err(_) => return Err(SmokeError("timed out connecting to Gate".to_string()).into()),
    };
    Ok((client, conn, receiver))
}

fn send<M>(conn: &Connection, msgid: MsgId, seq: u32, ack: u32, body: &M) -> Result<()>
where
    M: Message,
{
    let payload = CsPacket::encode_message(CsHead { msgid: msgid.as_u16(), seq, ack, ..Default::default() }, body)?;
    conn.send(payload).map_err(|error| SmokeError(format!("Gate send rejected for {msgid:?}: {error}")))?;
    Ok(())
}

async fn receive<M>(events: &mut mpsc::Receiver<Event>, expected: MsgId) -> Result<(CsHead, M)>
where
    M: Message + Default,
{
    let event = match timeout(EVENT_TIMEOUT, events.recv()).await {
        Ok(Some(event)) => event,
        Ok(None) => return Err(SmokeError("Gate event stream closed".to_string()).into()),
        Err(_) => return Err(SmokeError(format!("timed out waiting for {expected:?}")).into()),
    };
    let frame = match event {
        Event::Packet(frame) => frame,
        Event::Disconnected(session_id) => {
            return Err(SmokeError(format!("Gate disconnected session {session_id} before {expected:?}")).into());
        }
        Event::Connected(conn) => {
            return Err(SmokeError(format!("unexpected Gate connection {} before {expected:?}", conn.session_id())).into());
        }
    };
    let packet = CsPacket::decode(&frame.payload)?;
    if packet.head.msgid != expected.as_u16() {
        return Err(SmokeError(format!("expected {expected:?}, got message {}", packet.head.msgid)).into());
    }
    Ok((packet.head, M::decode(packet.body)?))
}

fn require_ok(status: Option<&pb::Status>, operation: &str) -> Result<()> {
    match status {
        Some(status) if status.code == code::OK => Ok(()),
        Some(status) => Err(SmokeError(format!("{operation} failed: code={} message={}", status.code, status.message)).into()),
        None => Err(SmokeError(format!("{operation} returned no status")).into()),
    }
}

fn require_ack(head: CsHead, expected: u32, operation: &str) -> Result<()> {
    if head.ack != expected || head.seq == 0 {
        return Err(SmokeError(format!("{operation} returned invalid sequence: seq={} ack={}", head.seq, head.ack)).into());
    }
    Ok(())
}
