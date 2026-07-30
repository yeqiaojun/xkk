use std::ops::RangeInclusive;

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

pub const OUTBOX_EXCLUDED_RANGES: &[RangeInclusive<u16>] = &[1..=99];

pub fn is_outbox_message(msgid: u16) -> bool {
    !OUTBOX_EXCLUDED_RANGES
        .iter()
        .any(|range| range.contains(&msgid))
}

pub fn response_for(request: MsgId) -> Option<MsgId> {
    Some(match request {
        MsgId::PingReq => MsgId::PingRsp,
        MsgId::LoginReq => MsgId::LoginRsp,
        MsgId::ReconnectReq => MsgId::ReconnectRsp,
        MsgId::LogoutReq => MsgId::LogoutRsp,
        MsgId::LogicLoginReq => MsgId::LogicLoginRsp,
        MsgId::KickSessionReq => MsgId::KickSessionRsp,
        MsgId::PlayerInfoReq => MsgId::PlayerInfoRsp,
        MsgId::UseItemReq => MsgId::UseItemRsp,
        MsgId::MailListReq => MsgId::MailListRsp,
        MsgId::MailReadReq => MsgId::MailReadRsp,
        MsgId::MailDeleteReq => MsgId::MailDeleteRsp,
        MsgId::MailClaimReq => MsgId::MailClaimRsp,
        MsgId::AddItemsReq => MsgId::AddItemsRsp,
        MsgId::RemoveItemsReq => MsgId::RemoveItemsRsp,
        MsgId::CheckItemsReq => MsgId::CheckItemsRsp,
        MsgId::SendMailReq => MsgId::SendMailRsp,
        MsgId::AuthLoginReq => MsgId::AuthLoginRsp,
        MsgId::AuthUseRoleReq => MsgId::AuthUseRoleRsp,
        MsgId::GamerInfoReq => MsgId::GamerInfoRsp,
        MsgId::ConfigKeyReq => MsgId::ConfigKeyRsp,
        MsgId::ConfigManifestReq => MsgId::ConfigManifestRsp,
        _ => return None,
    })
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
