use crate::MsgId;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageKind {
    Req,
    Rsp,
    Ntf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RouteTarget {
    Gate,
    Logic,
    Public,
}

impl MsgId {
    pub const fn as_u16(self) -> u16 {
        self as u16
    }

    pub const fn as_u32(self) -> u32 {
        self as u32
    }

    pub fn kind(self) -> Option<MessageKind> {
        let name = self.as_str_name();
        if name.ends_with("_REQ") {
            Some(MessageKind::Req)
        } else if name.ends_with("_RSP") {
            Some(MessageKind::Rsp)
        } else if name.ends_with("_NTF") {
            Some(MessageKind::Ntf)
        } else {
            None
        }
    }
}

pub fn is_outbox_message(msgid: u16) -> bool {
    if (1..xproto::SYSTEM_MSG_ID_END as u16).contains(&msgid) {
        return false;
    }
    !matches!(
        from_u16(msgid),
        Some(
            MsgId::PingRsp
                | MsgId::LoginRsp
                | MsgId::ReconnectRsp
                | MsgId::LogoutRsp
                | MsgId::KickNtf
        )
    )
}

pub fn response_for(request: MsgId) -> Option<MsgId> {
    let response = xproto::global_registry()
        .ok()?
        .response_id(request.as_u16())?;
    from_u16(response)
}

pub fn route_target(request: MsgId) -> Option<RouteTarget> {
    Some(match request {
        MsgId::PingReq | MsgId::LoginReq | MsgId::ReconnectReq | MsgId::LogoutReq => {
            RouteTarget::Gate
        }
        MsgId::PlayerInfoReq | MsgId::UseItemReq => RouteTarget::Logic,
        MsgId::MailListReq | MsgId::MailReadReq | MsgId::MailDeleteReq | MsgId::MailClaimReq => {
            RouteTarget::Public
        }
        _ => return None,
    })
}

pub fn from_u16(value: u16) -> Option<MsgId> {
    let id = MsgId::try_from(i32::from(value)).ok()?;
    (id != MsgId::Unspecified).then_some(id)
}
