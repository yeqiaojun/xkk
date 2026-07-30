use std::{ops::RangeInclusive, sync::Arc};

use thiserror::Error;
use xframe::xproto::registry::MessageRegistry;

pub mod pb {
    include!(concat!(env!("OUT_DIR"), "/xkk.v1.rs"));
    include!(concat!(env!("OUT_DIR"), "/xkk.xmongo.rs"));
    include!(concat!(env!("OUT_DIR"), "/model.xmongo.rs"));
}

pub use pb::MsgId;

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

#[derive(Debug, Error)]
pub enum ProtocolError {
    #[error(transparent)]
    Registry(#[from] xframe::xproto::Error),
    #[error("request message has no distinct response: {0:?}")]
    MissingResponse(MsgId),
}

pub fn validate_pair(request: MsgId, response: MsgId) -> Result<(), ProtocolError> {
    if response_for(request) != Some(response) {
        return Err(ProtocolError::MissingResponse(request));
    }
    Ok(())
}

pub fn client_registry() -> Result<Arc<MessageRegistry>, ProtocolError> {
    let mut registry = MessageRegistry::new();
    register_client_messages(&mut registry)?;
    Ok(Arc::new(registry))
}

fn register_client_messages(registry: &mut MessageRegistry) -> Result<(), ProtocolError> {
    macro_rules! register {
        ($id:ident, $ty:ty) => {
            registry.register::<$ty>(MsgId::$id.as_u16())?;
        };
    }

    register!(AckNtf, pb::AckNtf);
    register!(PingReq, pb::PingReq);
    register!(PingRsp, pb::PingRsp);
    register!(LoginReq, pb::LoginReq);
    register!(LoginRsp, pb::LoginRsp);
    register!(ReconnectReq, pb::ReconnectReq);
    register!(ReconnectRsp, pb::ReconnectRsp);
    register!(LogoutReq, pb::LogoutReq);
    register!(LogoutRsp, pb::LogoutRsp);
    register!(KickNtf, pb::KickNtf);
    register!(PlayerInfoReq, pb::PlayerInfoReq);
    register!(PlayerInfoRsp, pb::PlayerInfoRsp);
    register!(UseItemReq, pb::UseItemReq);
    register!(UseItemRsp, pb::UseItemRsp);
    register!(MailListReq, pb::MailListReq);
    register!(MailListRsp, pb::MailListRsp);
    register!(MailReadReq, pb::MailReadReq);
    register!(MailReadRsp, pb::MailReadRsp);
    register!(MailDeleteReq, pb::MailDeleteReq);
    register!(MailDeleteRsp, pb::MailDeleteRsp);
    register!(MailClaimReq, pb::MailClaimReq);
    register!(MailClaimRsp, pb::MailClaimRsp);
    register!(MailInfoNtf, pb::MailInfoNtf);
    Ok(())
}

pub mod code {
    pub const OK: i32 = 0;
    pub const INVALID_ARGUMENT: i32 = 1;
    pub const UNAUTHENTICATED: i32 = 2;
    pub const RATE_LIMITED: i32 = 3;
    pub const TEMPORARILY_UNAVAILABLE: i32 = 4;
    pub const RESUME_EXPIRED: i32 = 5;
    pub const SESSION_REPLACED: i32 = 6;
    pub const NOT_FOUND: i32 = 7;
    pub const CONFLICT: i32 = 8;
    pub const INSUFFICIENT_ITEMS: i32 = 9;
    pub const INTERNAL: i32 = 10;
    pub const OVERLOADED: i32 = 11;
}

pub fn ok_status() -> pb::Status {
    pb::Status {
        code: code::OK,
        message: String::new(),
    }
}

pub fn error_status(code: i32, message: impl Into<String>) -> pb::Status {
    pb::Status {
        code,
        message: message.into(),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;
    use xmongo::BsonPathGetter;

    #[test]
    fn msg_ids_are_generated_from_proto() {
        assert_eq!(MsgId::AckNtf.as_u16(), 1);
        assert_eq!(MsgId::PlayerInfoReq.as_u16(), 100);
        assert_eq!(MsgId::ConfigManifestRsp.as_u16(), 2105);
        assert_eq!(MsgId::PlayerInfoReq.as_str_name(), "PLAYER_INFO_REQ");
        assert_eq!(from_u16(100), Some(MsgId::PlayerInfoReq));
        assert_eq!(from_u16(99), None);
    }

    #[test]
    fn every_generated_request_has_a_distinct_response() {
        let mut seen = HashSet::new();
        for value in 1..=u16::MAX {
            let Some(request) = from_u16(value) else {
                continue;
            };
            assert!(seen.insert(request.as_u16()));
            if request.kind() != Some(MessageKind::Req) {
                continue;
            }
            let response = response_for(request).unwrap();
            assert_ne!(request, response);
            assert_eq!(response.kind(), Some(MessageKind::Rsp));
        }
    }

    #[test]
    fn only_control_range_bypasses_outbox() {
        assert!(!is_outbox_message(MsgId::LoginRsp.as_u16()));
        assert!(!is_outbox_message(MsgId::KickNtf.as_u16()));
        assert!(is_outbox_message(MsgId::PlayerInfoRsp.as_u16()));
        assert!(is_outbox_message(MsgId::MailInfoNtf.as_u16()));
    }

    #[test]
    fn client_registry_has_every_client_message() {
        let registry = client_registry().unwrap();
        let body = prost::Message::encode_to_vec(&pb::PingReq { client_time_ms: 7 });
        assert!(registry.decode(MsgId::PingReq.as_u16(), &body).is_ok());
    }

    #[test]
    fn generated_xmongo_traits_roundtrip_player_data() {
        let mut player = pb::PlayerData {
            gid: 1001,
            profile: Some(pb::PlayerInfo {
                gid: 1001,
                name: "tester".to_string(),
                level: 2,
                icon: 3,
                exp: 4,
            }),
            ..Default::default()
        };
        player.items.insert(2001, 7);

        let bson = player.bson_value().unwrap();
        let decoded = pb::PlayerData::from_bson_value(&bson).unwrap();

        assert_eq!(decoded, player);
    }
}
