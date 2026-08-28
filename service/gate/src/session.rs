use std::{
    collections::{HashMap, VecDeque},
    sync::{Arc, Mutex, RwLock},
    time::{Duration, Instant},
};

use prost::Message;
use thiserror::Error;
use xframe::{
    xnet::{Connection, SessionId},
    xproto::cs::{CsHead, CsPacket},
};
use xkk_protocol::{MsgId, is_outbox_message, pb};

#[derive(Debug, Clone, Copy)]
pub(crate) struct SessionConfig {
    pub outbox_messages: usize,
    pub resume_ttl: Duration,
    pub reconnect_total: usize,
    pub reconnect_window: Duration,
    pub reconnect_window_count: usize,
    pub request_window: Duration,
    pub request_window_count: usize,
    pub burst_window: Duration,
    pub burst_count: usize,
}

impl SessionConfig {
    // Gate session retention and rate limits are wire-behavior contracts. Exceeding them rejects
    // the request or drops the oldest resumable message and always emits an error log.
    pub const HARD_LIMITS: Self = Self {
        outbox_messages: 256,
        resume_ttl: Duration::from_secs(60),
        reconnect_total: 10,
        reconnect_window: Duration::from_secs(60),
        reconnect_window_count: 5,
        request_window: Duration::from_secs(5),
        request_window_count: 15,
        burst_window: Duration::from_secs(1),
        burst_count: 8,
    };

    pub fn validate(self) {
        assert!(
            self.outbox_messages > 0,
            "Gate outbox capacity must be positive"
        );
        assert!(
            !self.resume_ttl.is_zero(),
            "Gate resume TTL must be positive"
        );
        assert!(
            self.reconnect_total > 0,
            "Gate reconnect total must be positive"
        );
        assert!(
            self.reconnect_window_count > 0,
            "Gate reconnect window count must be positive"
        );
        assert!(
            self.reconnect_window_count <= self.reconnect_total,
            "Gate reconnect window count cannot exceed total"
        );
        assert!(
            !self.reconnect_window.is_zero(),
            "Gate reconnect window must be positive"
        );
        assert!(
            self.request_window_count > 0 && self.burst_count > 0,
            "Gate request limits must be positive"
        );
        assert!(
            self.burst_count <= self.request_window_count,
            "Gate burst limit cannot exceed request limit"
        );
        assert!(
            !self.request_window.is_zero() && !self.burst_window.is_zero(),
            "Gate request windows must be positive"
        );
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Routes {
    pub logic_id: i32,
    pub public_id: i32,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct StoreStats {
    pub players: usize,
    pub active: usize,
    pub offline: usize,
    pub outbox_messages: usize,
}

#[derive(Debug, Error, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RequestError {
    #[error("Gate session does not match")]
    SessionMismatch,
    #[error("client sequence is not newer")]
    InvalidSequence,
    #[error("client request rate exceeded")]
    RateLimited,
}

#[derive(Debug, Error, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ResumeError {
    #[error("Gate resume state is missing")]
    Missing,
    #[error("Gate resume state expired")]
    Expired,
    #[error("Gate previous session does not match")]
    SessionMismatch,
    #[error("Gate outbox no longer contains the requested sequence")]
    OutboxGap,
    #[error("Gate reconnect rate exceeded")]
    RateLimited,
}

#[derive(Debug, Error)]
pub(crate) enum SendError {
    #[error("Gate session does not match")]
    SessionMismatch,
    #[error("Gate connection write queue rejected the message")]
    QueueFull,
    #[error("Gate connection send failed: {0}")]
    Transport(#[source] xframe::xnet::Error),
    #[error(transparent)]
    Protocol(#[from] xframe::xproto::Error),
}

pub(crate) struct ReconnectResult {
    pub replay: Vec<Arc<[u8]>>,
    pub became_active: bool,
}

pub(crate) struct ClosingSession {
    pub gid: i64,
    pub session_id: SessionId,
    pub routes: Routes,
}

#[derive(Clone)]
pub(crate) struct ClientSessions {
    players: Arc<RwLock<HashMap<i64, Arc<Mutex<ClientSession>>>>>,
    config: SessionConfig,
}

impl ClientSessions {
    pub fn new(config: SessionConfig) -> Self {
        config.validate();
        Self {
            players: Arc::new(RwLock::new(HashMap::new())),
            config,
        }
    }

    pub fn install_login(
        &self,
        gid: i64,
        conn: Connection,
        routes: Routes,
        client_seq: u32,
    ) -> bool {
        let mut players = self.players.write().expect("Gate player map poisoned");
        let previous_active = players
            .get(&gid)
            .is_some_and(|session| session.lock().expect("Gate session poisoned").active());
        players.insert(
            gid,
            Arc::new(Mutex::new(ClientSession::new(
                conn,
                routes,
                client_seq,
                self.config,
            ))),
        );
        !previous_active
    }

    pub fn authorize_reconnect(
        &self,
        gid: i64,
        previous_session: SessionId,
        client_seq: u32,
        ack: u32,
        now: Instant,
    ) -> Result<Routes, ResumeError> {
        let session = self.get(gid).ok_or(ResumeError::Missing)?;
        let mut session = session.lock().expect("Gate session poisoned");
        session.authorize_reconnect(previous_session, client_seq, ack, now, self.config)
    }

    pub fn complete_reconnect(
        &self,
        gid: i64,
        previous_session: SessionId,
        conn: Connection,
        client_seq: u32,
        ack: u32,
        now: Instant,
    ) -> Result<ReconnectResult, ResumeError> {
        let session = self.get(gid).ok_or(ResumeError::Missing)?;
        let mut session = session.lock().expect("Gate session poisoned");
        session.complete_reconnect(previous_session, conn, client_seq, ack, now, self.config)
    }

    pub fn accept_request(
        &self,
        gid: i64,
        session_id: SessionId,
        seq: u32,
        ack: u32,
        now: Instant,
    ) -> Result<Routes, RequestError> {
        let session = self.get(gid).ok_or(RequestError::SessionMismatch)?;
        let mut session = session.lock().expect("Gate session poisoned");
        session.accept_request(session_id, seq, ack, now, self.config)
    }

    pub fn acknowledge(
        &self,
        gid: i64,
        session_id: SessionId,
        seq: u32,
        ack: u32,
        now: Instant,
    ) -> Result<(), RequestError> {
        let session = self.get(gid).ok_or(RequestError::SessionMismatch)?;
        let mut session = session.lock().expect("Gate session poisoned");
        session.accept_sequence(session_id, seq, ack, now, self.config.resume_ttl)
    }

    pub fn send<M>(
        &self,
        gid: i64,
        session_id: SessionId,
        msgid: MsgId,
        message: &M,
    ) -> Result<(), SendError>
    where
        M: Message,
    {
        let session = self.get(gid).ok_or(SendError::SessionMismatch)?;
        let mut session = session.lock().expect("Gate session poisoned");
        if session.session_id != session_id || !session.active() {
            return Err(SendError::SessionMismatch);
        }
        let payload = session.encode(msgid, message, Instant::now(), self.config)?;
        let conn = session
            .conn
            .as_ref()
            .expect("active Gate session has no connection");
        match conn.send_shared(payload) {
            Ok(()) => {}
            Err(xframe::xnet::Error::Backpressure { .. }) => return Err(SendError::QueueFull),
            Err(error) => return Err(SendError::Transport(error)),
        }
        Ok(())
    }

    pub fn current_session(&self, gid: i64) -> Option<SessionId> {
        let session = self.get(gid)?;
        let session = session.lock().expect("Gate session poisoned");
        session.active().then_some(session.session_id)
    }

    pub fn disconnect(&self, gid: i64, session_id: SessionId, now: Instant) -> Option<Routes> {
        let session = self.get(gid)?;
        let mut session = session.lock().expect("Gate session poisoned");
        if session.session_id != session_id || !session.active() {
            return None;
        }
        session.conn = None;
        session.disconnected_at = Some(now);
        Some(session.routes)
    }

    pub fn kick(&self, gid: i64, session_id: SessionId, code: i32, reason: &str) -> Option<bool> {
        let session = self.get(gid)?;
        let (conn, was_active) = {
            let mut session = session.lock().expect("Gate session poisoned");
            if session.session_id != session_id {
                return None;
            }
            let conn = session.conn.take();
            if let Some(conn) = conn.as_ref()
                && let Ok(payload) = session.encode(
                    MsgId::KickNtf,
                    &pb::KickNtf {
                        code,
                        reason: reason.to_string(),
                    },
                    Instant::now(),
                    self.config,
                )
                && let Err(error) = conn.send_shared(payload)
            {
                tracing::warn!(gid, session_id, %error, "Gate kick notification send failed");
            }
            let was_active = conn.is_some();
            (conn, was_active)
        };

        let mut players = self.players.write().expect("Gate player map poisoned");
        if players
            .get(&gid)
            .is_some_and(|current| Arc::ptr_eq(current, &session))
        {
            players.remove(&gid);
        }
        if let Some(conn) = conn {
            conn.close();
        }
        Some(was_active)
    }

    pub fn invalidate(&self, gid: i64, session_id: SessionId) -> bool {
        let mut players = self.players.write().expect("Gate player map poisoned");
        let Some(session) = players.get(&gid) else {
            return false;
        };
        let matches = session.lock().expect("Gate session poisoned").session_id == session_id;
        if matches {
            players.remove(&gid);
        }
        matches
    }

    pub fn prune_expired(&self, now: Instant) -> usize {
        let mut removed = 0;
        self.players
            .write()
            .expect("Gate player map poisoned")
            .retain(|_, session| {
                let session = session.lock().expect("Gate session poisoned");
                let keep = session.active()
                    || session
                        .disconnected_at
                        .is_some_and(|at| now.duration_since(at) <= self.config.resume_ttl);
                removed += usize::from(!keep);
                keep
            });
        removed
    }

    pub fn stats(&self) -> StoreStats {
        let players = self.players.read().expect("Gate player map poisoned");
        let mut stats = StoreStats {
            players: players.len(),
            ..Default::default()
        };
        for session in players.values() {
            let session = session.lock().expect("Gate session poisoned");
            if session.active() {
                stats.active += 1;
            } else {
                stats.offline += 1;
            }
            stats.outbox_messages += session.outbox.len();
        }
        stats
    }

    pub fn drain(&self) -> Vec<ClosingSession> {
        self.players
            .write()
            .expect("Gate player map poisoned")
            .drain()
            .map(|(gid, session)| {
                let mut session = session.lock().expect("Gate session poisoned");
                if let Some(conn) = session.conn.take() {
                    conn.close();
                }
                ClosingSession {
                    gid,
                    session_id: session.session_id,
                    routes: session.routes,
                }
            })
            .collect()
    }

    fn get(&self, gid: i64) -> Option<Arc<Mutex<ClientSession>>> {
        self.players
            .read()
            .expect("Gate player map poisoned")
            .get(&gid)
            .cloned()
    }
}

pub(crate) fn encode_direct<M>(
    msgid: MsgId,
    ack: u32,
    message: &M,
) -> xframe::xproto::Result<Arc<[u8]>>
where
    M: Message,
{
    Ok(Arc::from(
        CsPacket::encode_message(
            CsHead {
                msgid: msgid.as_u16(),
                ack,
                ..Default::default()
            },
            message,
        )?
        .into_boxed_slice(),
    ))
}

struct ClientSession {
    session_id: SessionId,
    conn: Option<Connection>,
    routes: Routes,
    server_seq: u32,
    client_seq: u32,
    outbox: VecDeque<OutboxMessage>,
    dropped_through: Option<u32>,
    disconnected_at: Option<Instant>,
    reconnect_count: usize,
    reconnect_times: VecDeque<Instant>,
    request_times: VecDeque<Instant>,
}

impl ClientSession {
    fn new(conn: Connection, routes: Routes, client_seq: u32, config: SessionConfig) -> Self {
        Self {
            session_id: conn.session_id(),
            conn: Some(conn),
            routes,
            server_seq: 0,
            client_seq,
            outbox: VecDeque::with_capacity(config.outbox_messages),
            dropped_through: None,
            disconnected_at: None,
            reconnect_count: 0,
            reconnect_times: VecDeque::with_capacity(config.reconnect_window_count),
            request_times: VecDeque::with_capacity(config.request_window_count),
        }
    }

    fn active(&self) -> bool {
        self.conn.is_some()
    }

    fn authorize_reconnect(
        &mut self,
        previous_session: SessionId,
        client_seq: u32,
        ack: u32,
        now: Instant,
        config: SessionConfig,
    ) -> Result<Routes, ResumeError> {
        if self.session_id != previous_session {
            return Err(ResumeError::SessionMismatch);
        }
        if !seq_after(client_seq, self.client_seq) {
            return Err(ResumeError::SessionMismatch);
        }
        if self
            .disconnected_at
            .is_some_and(|at| now.duration_since(at) > config.resume_ttl)
        {
            return Err(ResumeError::Expired);
        }
        self.prune_outbox(now, config.resume_ttl);
        if self
            .dropped_through
            .is_some_and(|dropped| !seq_at_or_after(ack, dropped))
        {
            return Err(ResumeError::OutboxGap);
        }
        if self.reconnect_count >= config.reconnect_total {
            tracing::error!(
                reconnect_count = self.reconnect_count,
                limit = config.reconnect_total,
                "Gate reconnect total hard limit exceeded"
            );
            return Err(ResumeError::RateLimited);
        }
        while self
            .reconnect_times
            .front()
            .is_some_and(|at| now.duration_since(*at) >= config.reconnect_window)
        {
            self.reconnect_times.pop_front();
        }
        if self.reconnect_times.len() >= config.reconnect_window_count {
            tracing::error!(
                reconnect_count = self.reconnect_times.len(),
                limit = config.reconnect_window_count,
                "Gate reconnect window hard limit exceeded"
            );
            return Err(ResumeError::RateLimited);
        }
        self.reconnect_count += 1;
        self.reconnect_times.push_back(now);
        Ok(self.routes)
    }

    fn complete_reconnect(
        &mut self,
        previous_session: SessionId,
        conn: Connection,
        client_seq: u32,
        ack: u32,
        now: Instant,
        config: SessionConfig,
    ) -> Result<ReconnectResult, ResumeError> {
        if self.session_id != previous_session {
            return Err(ResumeError::SessionMismatch);
        }
        if !seq_after(client_seq, self.client_seq) {
            return Err(ResumeError::SessionMismatch);
        }
        self.prune_outbox(now, config.resume_ttl);
        if self
            .dropped_through
            .is_some_and(|dropped| !seq_at_or_after(ack, dropped))
        {
            return Err(ResumeError::OutboxGap);
        }
        self.acknowledge_outbox(ack);
        let became_active = !self.active();
        self.session_id = conn.session_id();
        self.conn = Some(conn);
        self.client_seq = client_seq;
        self.disconnected_at = None;
        let replay = self
            .outbox
            .iter()
            .filter(|message| seq_after(message.seq, ack))
            .map(|message| message.payload.clone())
            .collect();
        Ok(ReconnectResult {
            replay,
            became_active,
        })
    }

    fn accept_request(
        &mut self,
        session_id: SessionId,
        seq: u32,
        ack: u32,
        now: Instant,
        config: SessionConfig,
    ) -> Result<Routes, RequestError> {
        self.accept_sequence(session_id, seq, ack, now, config.resume_ttl)?;
        if !self.accept_rate(now, config) {
            return Err(RequestError::RateLimited);
        }
        Ok(self.routes)
    }

    fn accept_rate(&mut self, now: Instant, config: SessionConfig) -> bool {
        while self
            .request_times
            .front()
            .is_some_and(|at| now.duration_since(*at) >= config.request_window)
        {
            self.request_times.pop_front();
        }
        if self.request_times.len() >= config.request_window_count {
            tracing::error!(
                request_count = self.request_times.len(),
                limit = config.request_window_count,
                "Gate request long-window hard limit exceeded"
            );
            return false;
        }
        let burst = self
            .request_times
            .iter()
            .rev()
            .take_while(|at| now.duration_since(**at) < config.burst_window)
            .count();
        if burst >= config.burst_count {
            tracing::error!(
                request_count = burst,
                limit = config.burst_count,
                "Gate request burst hard limit exceeded"
            );
            return false;
        }
        self.request_times.push_back(now);
        true
    }

    fn accept_sequence(
        &mut self,
        session_id: SessionId,
        seq: u32,
        ack: u32,
        now: Instant,
        resume_ttl: Duration,
    ) -> Result<(), RequestError> {
        if self.session_id != session_id || !self.active() {
            return Err(RequestError::SessionMismatch);
        }
        if !seq_after(seq, self.client_seq) {
            return Err(RequestError::InvalidSequence);
        }
        self.client_seq = seq;
        self.prune_outbox(now, resume_ttl);
        self.acknowledge_outbox(ack);
        Ok(())
    }

    fn encode<M>(
        &mut self,
        msgid: MsgId,
        message: &M,
        now: Instant,
        config: SessionConfig,
    ) -> xframe::xproto::Result<Arc<[u8]>>
    where
        M: Message,
    {
        self.server_seq = self.server_seq.wrapping_add(1);
        let payload: Arc<[u8]> = Arc::from(
            CsPacket::encode_message(
                CsHead {
                    msgid: msgid.as_u16(),
                    seq: self.server_seq,
                    ack: self.client_seq,
                    ..Default::default()
                },
                message,
            )?
            .into_boxed_slice(),
        );
        if is_outbox_message(msgid.as_u16()) {
            self.prune_outbox(now, config.resume_ttl);
            if self.outbox.len() == config.outbox_messages {
                tracing::error!(
                    retained = self.outbox.len(),
                    limit = config.outbox_messages,
                    "Gate outbox hard limit exceeded; dropping oldest message"
                );
                let dropped = self
                    .outbox
                    .pop_front()
                    .expect("full Gate outbox must have an oldest message");
                self.dropped_through = Some(dropped.seq);
            }
            self.outbox.push_back(OutboxMessage {
                seq: self.server_seq,
                sent_at: now,
                payload: payload.clone(),
            });
        }
        Ok(payload)
    }

    fn prune_outbox(&mut self, now: Instant, ttl: Duration) {
        while self
            .outbox
            .front()
            .is_some_and(|message| now.duration_since(message.sent_at) > ttl)
        {
            let dropped = self
                .outbox
                .pop_front()
                .expect("Gate outbox front disappeared");
            self.dropped_through = Some(dropped.seq);
        }
    }

    fn acknowledge_outbox(&mut self, ack: u32) {
        while self
            .outbox
            .front()
            .is_some_and(|message| !seq_after(message.seq, ack))
        {
            self.outbox.pop_front();
        }
    }
}

struct OutboxMessage {
    seq: u32,
    sent_at: Instant,
    payload: Arc<[u8]>,
}

fn seq_after(value: u32, previous: u32) -> bool {
    let distance = value.wrapping_sub(previous);
    distance != 0 && distance < (1_u32 << 31)
}

fn seq_at_or_after(value: u32, previous: u32) -> bool {
    value == previous || seq_after(value, previous)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> SessionConfig {
        SessionConfig {
            outbox_messages: 2,
            ..SessionConfig::HARD_LIMITS
        }
    }

    fn offline_session(now: Instant) -> ClientSession {
        ClientSession {
            session_id: 11,
            conn: None,
            routes: Routes {
                logic_id: 1,
                public_id: 2,
            },
            server_seq: 0,
            client_seq: 0,
            outbox: VecDeque::with_capacity(2),
            dropped_through: None,
            disconnected_at: Some(now),
            reconnect_count: 0,
            reconnect_times: VecDeque::new(),
            request_times: VecDeque::new(),
        }
    }

    #[test]
    fn sequence_comparison_wraps_at_u32() {
        assert!(seq_after(1, 0));
        assert!(!seq_after(1, 1));
        assert!(seq_after(0, u32::MAX));
        assert!(!seq_after(u32::MAX, 0));
    }

    #[test]
    fn evicted_unacked_message_makes_old_resume_fail() {
        let now = Instant::now();
        let mut session = offline_session(now);
        let cfg = config();
        session
            .encode(
                MsgId::PlayerInfoRsp,
                &pb::PlayerInfoRsp::default(),
                now,
                cfg,
            )
            .unwrap();
        session
            .encode(MsgId::UseItemRsp, &pb::UseItemRsp::default(), now, cfg)
            .unwrap();
        session
            .encode(MsgId::MailListRsp, &pb::MailListRsp::default(), now, cfg)
            .unwrap();

        assert_eq!(session.outbox.len(), 2);
        assert_eq!(
            session.authorize_reconnect(11, 1, 0, now, cfg),
            Err(ResumeError::OutboxGap)
        );
        assert!(session.authorize_reconnect(11, 1, 1, now, cfg).is_ok());
    }

    #[test]
    fn control_messages_do_not_enter_outbox() {
        let now = Instant::now();
        let mut session = offline_session(now);
        session
            .encode(MsgId::PingRsp, &pb::PingRsp::default(), now, config())
            .unwrap();
        assert!(session.outbox.is_empty());
    }

    #[test]
    fn reconnect_allows_five_per_window_and_ten_total() {
        let now = Instant::now();
        let mut session = offline_session(now);
        session.disconnected_at = None;
        let cfg = config();
        for offset in 0..5 {
            assert!(
                session
                    .authorize_reconnect(11, 1, 0, now + Duration::from_secs(offset), cfg)
                    .is_ok()
            );
        }
        assert_eq!(
            session.authorize_reconnect(11, 1, 0, now + Duration::from_secs(5), cfg),
            Err(ResumeError::RateLimited)
        );
        for offset in 0..5 {
            assert!(
                session
                    .authorize_reconnect(11, 1, 0, now + Duration::from_secs(61 + offset), cfg,)
                    .is_ok()
            );
        }
        assert_eq!(
            session.authorize_reconnect(11, 1, 0, now + Duration::from_secs(122), cfg),
            Err(ResumeError::RateLimited)
        );
    }

    #[test]
    fn request_limiter_enforces_burst_and_long_window() {
        let now = Instant::now();
        let mut session = offline_session(now);
        let cfg = config();

        for _ in 0..8 {
            assert!(session.accept_rate(now, cfg));
        }
        assert!(!session.accept_rate(now + Duration::from_millis(999), cfg));

        for _ in 0..7 {
            assert!(session.accept_rate(now + Duration::from_millis(1001), cfg));
        }
        assert!(!session.accept_rate(now + Duration::from_millis(1001), cfg));
        assert!(session.accept_rate(now + Duration::from_millis(5001), cfg));
    }

    #[test]
    fn drain_removes_offline_resume_state() {
        let sessions = ClientSessions::new(config());
        sessions
            .players
            .write()
            .unwrap()
            .insert(7, Arc::new(Mutex::new(offline_session(Instant::now()))));

        let drained = sessions.drain();

        assert_eq!(drained.len(), 1);
        assert_eq!(drained[0].gid, 7);
        assert_eq!(drained[0].session_id, 11);
        assert_eq!(sessions.stats(), StoreStats::default());
    }
}
