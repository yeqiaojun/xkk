use std::{
    collections::HashMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicI32, Ordering},
    },
    time::{Duration, Instant},
};

use prost::Message;
use tokio::{sync::mpsc, task::JoinHandle};
use xframe::{
    FrameHandle,
    xnet::{Connection, Frame as NetFrame, Handler, SessionId},
    xproto::cs::{CsHead, CsPacket},
    xproto::{RequestMessage, WireMessage},
    xrpc::{JsonMessage, RpcManager},
};
use xkk_cache::{OnlineData, clear_gate_by_session, load_online, save_online};
use xkk_common::{unix_millis, unix_seconds};
use xkk_protocol::{MsgId, RouteTarget, code, error_status, from_u16, ok_status, pb, route_target};
use xtoken::TokenCoder;
use xutil::{NetStatus, ServiceStatus};

use crate::{
    session::{ClientSessions, RequestError, ResumeError, Routes, SendError, encode_direct},
    stats::{LoginMetrics, LoginStats},
};

// These bounds define one Gate process. Hitting them rejects work and emits an error log; scaling
// is done by adding Gate instances rather than editing per-instance YAML.
const CLIENT_MAILBOX_CAPACITY: usize = 64;
const RPC_CALL_TIMEOUT: Duration = Duration::from_secs(3);
const SHUTDOWN_CLEANUP_CONCURRENCY: usize = 64;

#[derive(Debug, Clone)]
pub(crate) struct GatewaySettings {
    pub gate_id: i32,
    pub token_secret: String,
    pub token_expire_seconds: i64,
}

#[derive(Clone)]
pub(crate) struct Gateway {
    state: Arc<GatewayState>,
    workers: Arc<Mutex<HashMap<SessionId, ClientWorker>>>,
}

impl Gateway {
    pub fn new(
        frame: FrameHandle,
        redis: xredis::Client,
        sessions: ClientSessions,
        online_count: Arc<AtomicI32>,
        settings: GatewaySettings,
    ) -> Self {
        assert!(settings.token_expire_seconds > 0, "Gate token expiry must be positive");
        Self {
            state: Arc::new(GatewayState {
                frame,
                redis,
                sessions,
                token: TokenCoder::new(settings.token_secret, settings.token_expire_seconds),
                gate_id: settings.gate_id,
                rpc_timeout: RPC_CALL_TIMEOUT,
                online_count,
                login_metrics: LoginMetrics::default(),
                draining: AtomicBool::new(false),
            }),
            workers: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn register_rpc(&self, rpc: &RpcManager) -> xframe::xrpc::Result<()> {
        let kick_state = self.state.clone();
        rpc.register::<pb::KickSessionReq, _, _>(move |_ctx, request| {
            let state = kick_state.clone();
            async move { Ok(state.kick_session(request).await) }
        })?;

        let mail_state = self.state.clone();
        rpc.register_notification::<pb::MailPushNtf, _, _>(move |_ctx, request| {
            let state = mail_state.clone();
            async move {
                state.push_mail(request);
                Ok(())
            }
        })?;
        Ok(())
    }

    pub fn sessions(&self) -> ClientSessions {
        self.state.sessions.clone()
    }

    pub fn online_count(&self) -> i32 {
        self.state.online_count.load(Ordering::Acquire)
    }

    pub(crate) fn login_stats(&self) -> LoginStats {
        self.state.login_metrics.snapshot()
    }

    pub async fn shutdown(&self) {
        self.state.draining.store(true, Ordering::Release);
        let workers = self.workers.lock().expect("Gate worker map poisoned").drain().map(|(_, worker)| worker).collect::<Vec<_>>();
        for worker in &workers {
            worker.conn.close();
        }
        for worker in workers {
            drop(worker.sender);
            let _ = worker.task.await;
        }

        let sessions = self.state.sessions.drain();
        self.state.online_count.store(0, Ordering::Release);
        let mut cleanup = tokio::task::JoinSet::new();
        for session in sessions {
            if cleanup.len() == SHUTDOWN_CLEANUP_CONCURRENCY {
                let _ = cleanup.join_next().await;
            }
            let state = self.state.clone();
            cleanup.spawn(async move {
                state.cleanup_shutdown_session(session).await;
            });
        }
        while cleanup.join_next().await.is_some() {}

        let stats = self.state.sessions.stats();
        let rpc = self.state.frame.stats().rpc;
        let connection_workers = self.workers.lock().expect("Gate worker map poisoned").len();
        tracing::info!(
            online_players = self.online_count(),
            retained_players = stats.players,
            outbox_messages = stats.outbox_messages,
            connection_workers,
            rpc_pending = rpc.pending,
            rpc_inbound_active = rpc.inbound_active,
            "Gate client sessions drained"
        );
    }

    fn draining(&self) -> bool {
        self.state.draining.load(Ordering::Acquire)
    }
}

impl GatewayState {
    async fn cleanup_shutdown_session(&self, session: crate::session::ClosingSession) {
        if let Err(error) = self
            .frame
            .send_to(
                xkk_common::service_type::LOGIC,
                session.routes.logic_id,
                &pb::LogicDisconnectNtf { gid: session.gid, gate_id: self.gate_id, player_session: session.session_id },
            )
            .await
        {
            tracing::debug!(
                gid = session.gid,
                session_id = session.session_id,
                %error,
                "Gate shutdown Logic disconnect notification failed"
            );
        }
        if let Err(error) = clear_gate_by_session(&self.redis, session.gid, session.session_id, unix_seconds()).await {
            tracing::warn!(
                gid = session.gid,
                session_id = session.session_id,
                %error,
                "Gate shutdown Redis cleanup failed"
            );
        }
    }
}

impl Handler for Gateway {
    fn on_connected(&self, conn: Connection) {
        let mut workers = self.workers.lock().expect("Gate worker map poisoned");
        if self.draining() {
            conn.close();
            return;
        }
        let session_id = conn.session_id();
        let (sender, mut receiver) = mpsc::channel(CLIENT_MAILBOX_CAPACITY);
        let state = self.state.clone();
        let worker_conn = conn.clone();
        let task = tokio::spawn(async move {
            while let Some(frame) = receiver.recv().await {
                if worker_conn.is_closed() {
                    break;
                }
                state.handle_packet(frame).await;
            }
        });

        let old = workers.insert(session_id, ClientWorker { sender, task, conn: conn.clone() });
        assert!(old.is_none(), "Gate session worker registered twice");
        tracing::info!(
            session_id,
            peer_addr = %conn.peer_addr(),
            "Gate client connection established"
        );
    }

    fn on_packet(&self, frame: NetFrame) {
        if self.draining() {
            frame.conn.close();
            return;
        }
        let sender = self.workers.lock().expect("Gate worker map poisoned").get(&frame.session_id).map(|worker| worker.sender.clone());
        let Some(sender) = sender else {
            frame.conn.close();
            return;
        };
        if let Err(error) = sender.try_send(frame) {
            let frame = error.into_inner();
            tracing::error!(
                session_id = frame.session_id,
                peer_addr = %frame.peer_addr,
                limit = CLIENT_MAILBOX_CAPACITY,
                "Gate client mailbox hard limit exceeded"
            );
            frame.conn.close();
        }
    }

    fn on_disconnected(&self, conn: Connection) {
        if let Some(worker) = self.workers.lock().expect("Gate worker map poisoned").remove(&conn.session_id()) {
            worker.task.abort();
        }
        if self.draining() {
            return;
        }
        let state = self.state.clone();
        tokio::spawn(async move {
            state.disconnect(conn).await;
        });
    }
}

struct ClientWorker {
    sender: mpsc::Sender<NetFrame>,
    task: JoinHandle<()>,
    conn: Connection,
}

struct GatewayState {
    frame: FrameHandle,
    redis: xredis::Client,
    sessions: ClientSessions,
    token: TokenCoder,
    gate_id: i32,
    rpc_timeout: Duration,
    online_count: Arc<AtomicI32>,
    login_metrics: LoginMetrics,
    draining: AtomicBool,
}

impl GatewayState {
    async fn handle_packet(&self, frame: NetFrame) {
        let packet = match CsPacket::decode(&frame.payload) {
            Ok(packet) => packet,
            Err(error) => {
                tracing::warn!(
                    session_id = frame.session_id,
                    %error,
                    "Gate rejected invalid client packet"
                );
                frame.conn.close();
                return;
            }
        };
        let Some(msgid) = from_u16(packet.head.msgid) else {
            tracing::warn!(session_id = frame.session_id, msgid = packet.head.msgid, "Gate rejected unknown client message");
            frame.conn.close();
            return;
        };

        match msgid {
            MsgId::AckNtf => self.handle_ack(&frame, packet).await,
            MsgId::PingReq => self.handle_ping(&frame, packet).await,
            MsgId::LoginReq => self.handle_login(&frame, packet).await,
            MsgId::ReconnectReq => self.handle_reconnect(&frame, packet).await,
            MsgId::LogoutReq => self.handle_logout(&frame, packet).await,
            MsgId::PlayerInfoReq => {
                let Some(request) = decode::<pb::PlayerInfoReq>(&frame, packet.body) else {
                    return;
                };
                self.forward(&frame, packet.head, RouteTarget::Logic, MsgId::PlayerInfoReq, MsgId::PlayerInfoRsp, request, |status| {
                    pb::PlayerInfoRsp { status: Some(status), ..Default::default() }
                })
                .await;
            }
            MsgId::UseItemReq => {
                let Some(request) = decode::<pb::UseItemReq>(&frame, packet.body) else {
                    return;
                };
                self.forward(&frame, packet.head, RouteTarget::Logic, MsgId::UseItemReq, MsgId::UseItemRsp, request, |status| {
                    pb::UseItemRsp { status: Some(status), ..Default::default() }
                })
                .await;
            }
            MsgId::MailListReq => {
                let Some(request) = decode::<pb::MailListReq>(&frame, packet.body) else {
                    return;
                };
                self.forward(&frame, packet.head, RouteTarget::Public, MsgId::MailListReq, MsgId::MailListRsp, request, |status| {
                    pb::MailListRsp { status: Some(status), ..Default::default() }
                })
                .await;
            }
            MsgId::MailReadReq => {
                let Some(request) = decode::<pb::MailReadReq>(&frame, packet.body) else {
                    return;
                };
                self.forward(&frame, packet.head, RouteTarget::Public, MsgId::MailReadReq, MsgId::MailReadRsp, request, |status| {
                    pb::MailReadRsp { status: Some(status), ..Default::default() }
                })
                .await;
            }
            MsgId::MailDeleteReq => {
                let Some(request) = decode::<pb::MailDeleteReq>(&frame, packet.body) else {
                    return;
                };
                self.forward(&frame, packet.head, RouteTarget::Public, MsgId::MailDeleteReq, MsgId::MailDeleteRsp, request, |status| {
                    pb::MailDeleteRsp { status: Some(status), ..Default::default() }
                })
                .await;
            }
            MsgId::MailClaimReq => {
                let Some(request) = decode::<pb::MailClaimReq>(&frame, packet.body) else {
                    return;
                };
                self.forward(&frame, packet.head, RouteTarget::Public, MsgId::MailClaimReq, MsgId::MailClaimRsp, request, |status| {
                    pb::MailClaimRsp { status: Some(status), ..Default::default() }
                })
                .await;
            }
            _ => {
                tracing::warn!(session_id = frame.session_id, msgid = msgid.as_u16(), "Gate rejected non-request client message");
                frame.conn.close();
            }
        }
    }

    async fn handle_ack(&self, frame: &NetFrame, packet: CsPacket<'_>) {
        if decode::<pb::AckNtf>(frame, packet.body).is_none() {
            return;
        }
        let Some(gid) = frame.conn.user_id() else {
            frame.conn.close();
            return;
        };
        if self.sessions.acknowledge(gid, frame.session_id, packet.head.seq, packet.head.ack, Instant::now()).is_err() {
            frame.conn.close();
        }
    }

    async fn handle_ping(&self, frame: &NetFrame, packet: CsPacket<'_>) {
        let Some(request) = decode::<pb::PingReq>(frame, packet.body) else {
            return;
        };
        let response = pb::PingRsp { status: Some(ok_status()), client_time_ms: request.client_time_ms, server_time_ms: unix_millis() };
        let Some(gid) = frame.conn.user_id() else {
            self.send_direct(&frame.conn, MsgId::PingRsp, packet.head.seq, &response, false);
            return;
        };
        if self.sessions.acknowledge(gid, frame.session_id, packet.head.seq, packet.head.ack, Instant::now()).is_err() {
            frame.conn.close();
            return;
        }
        self.send_bound(gid, frame.session_id, MsgId::PingRsp, &response);
    }

    async fn handle_login(&self, frame: &NetFrame, packet: CsPacket<'_>) {
        let total_started = Instant::now();
        let Some(request) = decode::<pb::LoginReq>(frame, packet.body) else {
            return;
        };
        if frame.conn.user_id().is_some()
            || request.gid <= 0
            || request.token.is_empty()
            || request.device_id.len() < 8
            || packet.head.seq == 0
        {
            self.reject_login(&frame.conn, packet.head.seq, error_status(code::INVALID_ARGUMENT, "invalid login request"));
            return;
        }

        let Some(mut online) = self.verify_online(request.gid, &request.token, &request.device_id).await else {
            self.reject_login(&frame.conn, packet.head.seq, error_status(code::UNAUTHENTICATED, "token verification failed"));
            return;
        };
        let route_started = Instant::now();
        let logic_id = match self.select_logic(&online) {
            Some(logic_id) => logic_id,
            None => {
                self.reject_login(&frame.conn, packet.head.seq, error_status(code::TEMPORARILY_UNAVAILABLE, "Logic unavailable"));
                return;
            }
        };
        let public_id = match self.frame.pick_by_hash(xkk_common::service_type::PUBLIC, request.gid) {
            Ok(instance) => instance.instance_id,
            Err(error) => {
                tracing::warn!(gid = request.gid, %error, "Gate Public selection failed");
                self.reject_login(&frame.conn, packet.head.seq, error_status(code::TEMPORARILY_UNAVAILABLE, "Public unavailable"));
                return;
            }
        };
        self.login_metrics.route_select.record(route_started.elapsed());

        let logic_request = pb::LogicLoginReq {
            gid: request.gid,
            gate_id: self.gate_id,
            player_session: frame.session_id,
            device_id: request.device_id.clone(),
            reconnect: false,
        };
        let route = xkk_common::RouteIdentity::from_signed(request.gid, frame.session_id).expect("validated login route is positive");
        let rpc_started = Instant::now();
        let logic_response = self
            .frame
            .call_routed_to(xkk_common::service_type::LOGIC, logic_id, route.key(), route.session(), &logic_request, self.rpc_timeout)
            .await;
        self.login_metrics.logic_rpc.record(rpc_started.elapsed());
        let logic_response: pb::LogicLoginRsp = match logic_response {
            Ok(response) => response,
            Err(error) => {
                tracing::warn!(gid = request.gid, logic_id, %error, "Gate Logic login RPC failed");
                self.reject_login(&frame.conn, packet.head.seq, error_status(code::TEMPORARILY_UNAVAILABLE, "Logic login failed"));
                return;
            }
        };
        let status = logic_response.status.clone().unwrap_or_else(|| error_status(code::INTERNAL, "Logic login response has no status"));
        if status.code != code::OK {
            self.reject_login(&frame.conn, packet.head.seq, status);
            return;
        }

        let bind_started = Instant::now();
        let routes = Routes { logic_id, public_id };
        if !frame.conn.bind_user(request.gid) || frame.conn.is_closed() {
            frame.conn.close();
            return;
        }
        let became_active = self.sessions.install_login(request.gid, frame.conn.clone(), routes, packet.head.seq);
        if became_active {
            self.online_count.fetch_add(1, Ordering::AcqRel);
        }
        self.login_metrics.session_bind.record(bind_started.elapsed());

        online.session = frame.session_id;
        online.login_time = unix_seconds();
        online.logout_time = 0;
        online.public_id = public_id;
        online.gate_id = self.gate_id;
        online.logic_id = logic_id;
        let save_started = Instant::now();
        let save_result = save_online(&self.redis, &online).await;
        self.login_metrics.redis_save_online.record(save_started.elapsed());
        if let Err(error) = save_result {
            tracing::error!(gid = request.gid, %error, "Gate online save failed after login");
            self.kick_local(request.gid, frame.session_id, code::INTERNAL, "online save failed").await;
            return;
        }

        let send_started = Instant::now();
        let response = pb::LoginRsp {
            status: Some(ok_status()),
            gid: request.gid,
            logic_id,
            public_id,
            player: logic_response.player,
            items: logic_response.items,
            session_id: frame.session_id,
        };
        self.send_bound(request.gid, frame.session_id, MsgId::LoginRsp, &response);
        self.login_metrics.response_send.record(send_started.elapsed());
        self.login_metrics.total.record(total_started.elapsed());
    }

    async fn handle_reconnect(&self, frame: &NetFrame, packet: CsPacket<'_>) {
        let Some(request) = decode::<pb::ReconnectReq>(frame, packet.body) else {
            return;
        };
        if frame.conn.user_id().is_some()
            || request.gid <= 0
            || request.previous_session <= 0
            || request.token.is_empty()
            || request.device_id.len() < 8
            || request.ack != packet.head.ack
            || packet.head.seq == 0
        {
            self.reject_reconnect(&frame.conn, packet.head.seq, error_status(code::INVALID_ARGUMENT, "invalid reconnect request"));
            return;
        }
        let Some(mut online) = self.verify_online(request.gid, &request.token, &request.device_id).await else {
            self.reject_reconnect(&frame.conn, packet.head.seq, error_status(code::UNAUTHENTICATED, "token verification failed"));
            return;
        };
        let routes =
            match self.sessions.authorize_reconnect(request.gid, request.previous_session, packet.head.seq, request.ack, Instant::now()) {
                Ok(routes) => routes,
                Err(error) => {
                    let status = match error {
                        ResumeError::RateLimited => error_status(code::RATE_LIMITED, "reconnect rate exceeded"),
                        _ => error_status(code::RESUME_EXPIRED, "resume state expired"),
                    };
                    self.reject_reconnect(&frame.conn, packet.head.seq, status);
                    return;
                }
            };
        if online.logic_id != routes.logic_id {
            self.reject_reconnect(&frame.conn, packet.head.seq, error_status(code::RESUME_EXPIRED, "Logic ownership changed"));
            return;
        }

        let logic_request = pb::LogicLoginReq {
            gid: request.gid,
            gate_id: self.gate_id,
            player_session: frame.session_id,
            device_id: request.device_id,
            reconnect: true,
        };
        let route = xkk_common::RouteIdentity::from_signed(request.gid, frame.session_id).expect("validated reconnect route is positive");
        let logic_response: pb::LogicLoginRsp = match self
            .frame
            .call_routed_to(
                xkk_common::service_type::LOGIC,
                routes.logic_id,
                route.key(),
                route.session(),
                &logic_request,
                self.rpc_timeout,
            )
            .await
        {
            Ok(response) => response,
            Err(error) => {
                tracing::warn!(gid = request.gid, %error, "Gate Logic reconnect RPC failed");
                self.reject_reconnect(&frame.conn, packet.head.seq, error_status(code::TEMPORARILY_UNAVAILABLE, "Logic reconnect failed"));
                return;
            }
        };
        let status = logic_response.status.unwrap_or_else(|| error_status(code::INTERNAL, "Logic reconnect response has no status"));
        if status.code != code::OK {
            self.reject_reconnect(&frame.conn, packet.head.seq, status);
            return;
        }

        if !frame.conn.bind_user(request.gid) || frame.conn.is_closed() {
            frame.conn.close();
            return;
        }
        let reconnect = match self.sessions.complete_reconnect(
            request.gid,
            request.previous_session,
            frame.conn.clone(),
            packet.head.seq,
            request.ack,
            Instant::now(),
        ) {
            Ok(reconnect) => reconnect,
            Err(_) => {
                frame.conn.close();
                return;
            }
        };
        if reconnect.became_active {
            self.online_count.fetch_add(1, Ordering::AcqRel);
        }

        online.session = frame.session_id;
        online.login_time = unix_seconds();
        online.logout_time = 0;
        online.public_id = routes.public_id;
        online.gate_id = self.gate_id;
        if let Err(error) = save_online(&self.redis, &online).await {
            tracing::error!(gid = request.gid, %error, "Gate online save failed after reconnect");
            self.kick_local(request.gid, frame.session_id, code::INTERNAL, "online save failed").await;
            return;
        }

        for payload in &reconnect.replay {
            if let Err(error) = frame.conn.send_shared(payload.clone()) {
                tracing::warn!(
                    gid = request.gid,
                    session_id = frame.session_id,
                    %error,
                    "Gate reconnect replay send failed"
                );
                self.kick_local(request.gid, frame.session_id, code::OVERLOADED, "replay queue full").await;
                return;
            }
        }
        let response =
            pb::ReconnectRsp { status: Some(ok_status()), replayed: reconnect.replay.len() as u32, session_id: frame.session_id };
        self.send_bound(request.gid, frame.session_id, MsgId::ReconnectRsp, &response);
    }

    async fn handle_logout(&self, frame: &NetFrame, packet: CsPacket<'_>) {
        let Some(_request) = decode::<pb::LogoutReq>(frame, packet.body) else {
            return;
        };
        let Some(gid) = frame.conn.user_id() else {
            frame.conn.close();
            return;
        };
        let routes = match self.sessions.accept_request(gid, frame.session_id, packet.head.seq, packet.head.ack, Instant::now()) {
            Ok(routes) => routes,
            Err(RequestError::RateLimited) => {
                self.send_bound(
                    gid,
                    frame.session_id,
                    MsgId::LogoutRsp,
                    &pb::LogoutRsp { status: Some(error_status(code::RATE_LIMITED, "request rate exceeded")) },
                );
                return;
            }
            Err(_) => {
                frame.conn.close();
                return;
            }
        };

        self.send_bound(gid, frame.session_id, MsgId::LogoutRsp, &pb::LogoutRsp { status: Some(ok_status()) });
        self.sessions.invalidate(gid, frame.session_id);
        self.decrement_online();
        let _ = self
            .frame
            .send_to(
                xkk_common::service_type::LOGIC,
                routes.logic_id,
                &pb::LogicDisconnectNtf { gid, gate_id: self.gate_id, player_session: frame.session_id },
            )
            .await;
        let _ = clear_gate_by_session(&self.redis, gid, frame.session_id, unix_seconds()).await;
        frame.conn.close();
    }

    #[allow(clippy::too_many_arguments)]
    async fn forward<Req, F>(
        &self,
        frame: &NetFrame,
        head: CsHead,
        target: RouteTarget,
        request_msgid: MsgId,
        response_msgid: MsgId,
        request: Req,
        make_error: F,
    ) where
        Req: RequestMessage + Send + Sync,
        Req::Response: JsonMessage + Send,
        F: FnOnce(pb::Status) -> Req::Response,
    {
        debug_assert_eq!(request_msgid.as_u16(), Req::ID);
        debug_assert_eq!(response_msgid.as_u16(), Req::Response::ID);
        let Some(gid) = frame.conn.user_id() else {
            frame.conn.close();
            return;
        };
        let routes = match self.sessions.accept_request(gid, frame.session_id, head.seq, head.ack, Instant::now()) {
            Ok(routes) => routes,
            Err(RequestError::RateLimited) => {
                let response = make_error(error_status(code::RATE_LIMITED, "request rate exceeded"));
                self.send_bound(gid, frame.session_id, response_msgid, &response);
                return;
            }
            Err(_) => {
                frame.conn.close();
                return;
            }
        };
        let server_id = match target {
            RouteTarget::Logic => routes.logic_id,
            RouteTarget::Public => routes.public_id,
            RouteTarget::Gate => unreachable!("Gate-local messages are not forwarded"),
        };
        let service_type = match target {
            RouteTarget::Logic => xkk_common::service_type::LOGIC,
            RouteTarget::Public => xkk_common::service_type::PUBLIC,
            RouteTarget::Gate => unreachable!("Gate-local messages are not forwarded"),
        };
        debug_assert_eq!(route_target(request_msgid), Some(target));
        let route = xkk_common::RouteIdentity::from_signed(gid, frame.session_id).expect("bound request route is positive");

        let response: Req::Response =
            match self.frame.call_routed_to(service_type, server_id, route.key(), route.session(), &request, self.rpc_timeout).await {
                Ok(response) => response,
                Err(error) => {
                    tracing::warn!(
                        gid,
                        session_id = frame.session_id,
                        server_id,
                        msgid = request_msgid.as_u16(),
                        %error,
                        "Gate business RPC failed"
                    );
                    make_error(error_status(code::TEMPORARILY_UNAVAILABLE, "target service unavailable"))
                }
            };
        self.send_bound(gid, frame.session_id, response_msgid, &response);
    }

    async fn verify_online(&self, gid: i64, token: &str, device_id: &str) -> Option<OnlineData> {
        let token_started = Instant::now();
        let decoded_gid = self.token.simple_token_decode(token, device_id).ok();
        self.login_metrics.token_decode.record(token_started.elapsed());
        if decoded_gid? != gid {
            return None;
        }
        let redis_started = Instant::now();
        let online = load_online(&self.redis, gid).await;
        self.login_metrics.redis_load_online.record(redis_started.elapsed());
        let online = match online {
            Ok(Some(online)) => online,
            Ok(None) => return None,
            Err(error) => {
                tracing::warn!(gid, %error, "Gate Redis online load failed");
                return None;
            }
        };
        (online.gid == gid && online.token == token).then_some(online)
    }

    fn select_logic(&self, online: &OnlineData) -> Option<i32> {
        if online.logic_id == 0 {
            return self.frame.pick_min_online_and_increment(xkk_common::service_type::LOGIC).ok().map(|instance| instance.instance_id);
        }
        let instance = self.frame.service_instance(xkk_common::service_type::LOGIC, online.logic_id).ok()?;
        (instance.enable && instance.healthy == ServiceStatus::Health && instance.net_status == NetStatus::Connected)
            .then_some(instance.instance_id)
    }

    async fn disconnect(&self, conn: Connection) {
        let session_id = conn.session_id();
        let Some(gid) = conn.user_id() else {
            tracing::info!(session_id, peer_addr = %conn.peer_addr(), "Gate client connection closed");
            return;
        };
        let Some(routes) = self.sessions.disconnect(gid, session_id, Instant::now()) else {
            return;
        };
        self.decrement_online();
        if let Err(error) = self
            .frame
            .send_to(
                xkk_common::service_type::LOGIC,
                routes.logic_id,
                &pb::LogicDisconnectNtf { gid, gate_id: self.gate_id, player_session: session_id },
            )
            .await
        {
            tracing::debug!(gid, session_id, %error, "Gate Logic disconnect notification failed");
        }
        if let Err(error) = clear_gate_by_session(&self.redis, gid, session_id, unix_seconds()).await {
            tracing::warn!(gid, session_id, %error, "Gate Redis disconnect cleanup failed");
        }
        tracing::info!(gid, session_id, peer_addr = %conn.peer_addr(), "Gate client connection closed");
    }

    async fn kick_session(&self, request: pb::KickSessionReq) -> pb::KickSessionRsp {
        if request.gid <= 0 || request.player_session <= 0 {
            return pb::KickSessionRsp { status: Some(error_status(code::INVALID_ARGUMENT, "invalid kick target")), kicked: false };
        }
        let result = self.sessions.kick(request.gid, request.player_session, request.code, &request.reason);
        if result == Some(true) {
            self.decrement_online();
        }
        if result.is_some() {
            let _ = clear_gate_by_session(&self.redis, request.gid, request.player_session, unix_seconds()).await;
        }
        pb::KickSessionRsp { status: Some(ok_status()), kicked: result.is_some() }
    }

    fn push_mail(&self, request: pb::MailPushNtf) {
        let Some(mail) = request.mail else {
            return;
        };
        let Some(session_id) = self.sessions.current_session(request.gid) else {
            return;
        };
        self.send_bound(request.gid, session_id, MsgId::MailInfoNtf, &pb::MailInfoNtf { mails: vec![mail] });
    }

    async fn kick_local(&self, gid: i64, session_id: SessionId, code: i32, reason: &str) {
        if self.sessions.kick(gid, session_id, code, reason) == Some(true) {
            self.decrement_online();
        }
        let _ = clear_gate_by_session(&self.redis, gid, session_id, unix_seconds()).await;
    }

    fn reject_login(&self, conn: &Connection, ack: u32, status: pb::Status) {
        self.send_direct(conn, MsgId::LoginRsp, ack, &pb::LoginRsp { status: Some(status), ..Default::default() }, true);
    }

    fn reject_reconnect(&self, conn: &Connection, ack: u32, status: pb::Status) {
        self.send_direct(conn, MsgId::ReconnectRsp, ack, &pb::ReconnectRsp { status: Some(status), replayed: 0, session_id: 0 }, true);
    }

    fn send_direct<M>(&self, conn: &Connection, msgid: MsgId, ack: u32, message: &M, close: bool)
    where
        M: Message,
    {
        match encode_direct(msgid, ack, message) {
            Ok(payload) => {
                if let Err(error) = conn.send_shared(payload) {
                    tracing::warn!(%error, msgid = msgid.as_u16(), "Gate direct response send failed");
                }
            }
            Err(error) => {
                tracing::error!(%error, msgid = msgid.as_u16(), "Gate response encode failed");
            }
        }
        if close {
            conn.close();
        }
    }

    fn send_bound<M>(&self, gid: i64, session_id: SessionId, msgid: MsgId, message: &M)
    where
        M: Message,
    {
        if let Err(error) = self.sessions.send(gid, session_id, msgid, message) {
            match error {
                SendError::SessionMismatch => {}
                SendError::QueueFull => {
                    tracing::warn!(gid, session_id, msgid = msgid.as_u16(), "Gate write queue full");
                }
                SendError::Transport(error) => {
                    tracing::warn!(gid, session_id, msgid = msgid.as_u16(), %error, "Gate response send failed");
                }
                SendError::Protocol(error) => {
                    tracing::error!(gid, session_id, msgid = msgid.as_u16(), %error, "Gate response encode failed");
                }
            }
        }
    }

    fn decrement_online(&self) {
        let _ = self.online_count.fetch_update(Ordering::AcqRel, Ordering::Acquire, |current| Some((current - 1).max(0)));
    }
}

fn decode<M>(frame: &NetFrame, body: &[u8]) -> Option<M>
where
    M: Message + Default,
{
    match M::decode(body) {
        Ok(message) => Some(message),
        Err(error) => {
            tracing::warn!(
                session_id = frame.session_id,
                %error,
                "Gate protobuf decode failed"
            );
            frame.conn.close();
            None
        }
    }
}
