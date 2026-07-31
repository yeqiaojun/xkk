use xkk_common::{LatencyRecorder, LatencyStats};

#[derive(Default)]
pub(crate) struct LoginMetrics {
    pub(crate) token_decode: LatencyRecorder,
    pub(crate) redis_load_online: LatencyRecorder,
    pub(crate) route_select: LatencyRecorder,
    pub(crate) logic_rpc: LatencyRecorder,
    pub(crate) session_bind: LatencyRecorder,
    pub(crate) redis_save_online: LatencyRecorder,
    pub(crate) response_send: LatencyRecorder,
    pub(crate) total: LatencyRecorder,
}

#[derive(Clone, Copy)]
pub(crate) struct LoginStats {
    pub(crate) token_decode: LatencyStats,
    pub(crate) redis_load_online: LatencyStats,
    pub(crate) route_select: LatencyStats,
    pub(crate) logic_rpc: LatencyStats,
    pub(crate) session_bind: LatencyStats,
    pub(crate) redis_save_online: LatencyStats,
    pub(crate) response_send: LatencyStats,
    pub(crate) total: LatencyStats,
}

impl LoginMetrics {
    pub(crate) fn snapshot(&self) -> LoginStats {
        LoginStats {
            token_decode: self.token_decode.snapshot(),
            redis_load_online: self.redis_load_online.snapshot(),
            route_select: self.route_select.snapshot(),
            logic_rpc: self.logic_rpc.snapshot(),
            session_bind: self.session_bind.snapshot(),
            redis_save_online: self.redis_save_online.snapshot(),
            response_send: self.response_send.snapshot(),
            total: self.total.snapshot(),
        }
    }
}
