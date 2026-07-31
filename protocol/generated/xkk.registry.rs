impl xproto::WireMessage for pb::PlayerInfoReq {
    const ID: u16 = 100;
}
impl xproto::WireMessage for pb::PlayerInfoRsp {
    const ID: u16 = 101;
}
impl xproto::WireMessage for pb::UseItemReq {
    const ID: u16 = 102;
}
impl xproto::WireMessage for pb::UseItemRsp {
    const ID: u16 = 103;
}
impl xproto::WireMessage for pb::MailListReq {
    const ID: u16 = 110;
}
impl xproto::WireMessage for pb::MailListRsp {
    const ID: u16 = 111;
}
impl xproto::WireMessage for pb::MailReadReq {
    const ID: u16 = 112;
}
impl xproto::WireMessage for pb::MailReadRsp {
    const ID: u16 = 113;
}
impl xproto::WireMessage for pb::MailDeleteReq {
    const ID: u16 = 114;
}
impl xproto::WireMessage for pb::MailDeleteRsp {
    const ID: u16 = 115;
}
impl xproto::WireMessage for pb::MailClaimReq {
    const ID: u16 = 116;
}
impl xproto::WireMessage for pb::MailClaimRsp {
    const ID: u16 = 117;
}
impl xproto::WireMessage for pb::MailInfoNtf {
    const ID: u16 = 118;
}
impl xproto::WireMessage for pb::AddItemsReq {
    const ID: u16 = 1000;
}
impl xproto::WireMessage for pb::AddItemsRsp {
    const ID: u16 = 1001;
}
impl xproto::WireMessage for pb::RemoveItemsReq {
    const ID: u16 = 1002;
}
impl xproto::WireMessage for pb::RemoveItemsRsp {
    const ID: u16 = 1003;
}
impl xproto::WireMessage for pb::CheckItemsReq {
    const ID: u16 = 1004;
}
impl xproto::WireMessage for pb::CheckItemsRsp {
    const ID: u16 = 1005;
}
impl xproto::WireMessage for pb::SendMailReq {
    const ID: u16 = 1010;
}
impl xproto::WireMessage for pb::SendMailRsp {
    const ID: u16 = 1011;
}
impl xproto::WireMessage for pb::MailPushNtf {
    const ID: u16 = 1012;
}
impl xproto::WireMessage for pb::AuthLoginReq {
    const ID: u16 = 2000;
}
impl xproto::WireMessage for pb::AuthLoginRsp {
    const ID: u16 = 2001;
}
impl xproto::WireMessage for pb::AuthUseRoleReq {
    const ID: u16 = 2002;
}
impl xproto::WireMessage for pb::AuthUseRoleRsp {
    const ID: u16 = 2003;
}
impl xproto::WireMessage for pb::GamerInfoReq {
    const ID: u16 = 2100;
}
impl xproto::WireMessage for pb::GamerInfoRsp {
    const ID: u16 = 2101;
}
impl xproto::WireMessage for pb::ConfigKeyReq {
    const ID: u16 = 2102;
}
impl xproto::WireMessage for pb::ConfigKeyRsp {
    const ID: u16 = 2103;
}
impl xproto::WireMessage for pb::ConfigManifestReq {
    const ID: u16 = 2104;
}
impl xproto::WireMessage for pb::ConfigManifestRsp {
    const ID: u16 = 2105;
}
impl xproto::WireMessage for pb::AckNtf {
    const ID: u16 = 3000;
}
impl xproto::WireMessage for pb::PingReq {
    const ID: u16 = 3001;
}
impl xproto::WireMessage for pb::PingRsp {
    const ID: u16 = 3002;
}
impl xproto::WireMessage for pb::LoginReq {
    const ID: u16 = 3003;
}
impl xproto::WireMessage for pb::LoginRsp {
    const ID: u16 = 3004;
}
impl xproto::WireMessage for pb::ReconnectReq {
    const ID: u16 = 3005;
}
impl xproto::WireMessage for pb::ReconnectRsp {
    const ID: u16 = 3006;
}
impl xproto::WireMessage for pb::LogoutReq {
    const ID: u16 = 3007;
}
impl xproto::WireMessage for pb::LogoutRsp {
    const ID: u16 = 3008;
}
impl xproto::WireMessage for pb::KickNtf {
    const ID: u16 = 3009;
}
impl xproto::WireMessage for pb::LogicLoginReq {
    const ID: u16 = 3010;
}
impl xproto::WireMessage for pb::LogicLoginRsp {
    const ID: u16 = 3011;
}
impl xproto::WireMessage for pb::LogicDisconnectNtf {
    const ID: u16 = 3012;
}
impl xproto::WireMessage for pb::KickSessionReq {
    const ID: u16 = 3013;
}
impl xproto::WireMessage for pb::KickSessionRsp {
    const ID: u16 = 3014;
}
impl xproto::RequestMessage for pb::PlayerInfoReq {
    type Response = pb::PlayerInfoRsp;
}
impl xproto::ResponseMessage for pb::PlayerInfoRsp {
    type Request = pb::PlayerInfoReq;
}
impl xproto::RequestMessage for pb::UseItemReq {
    type Response = pb::UseItemRsp;
}
impl xproto::ResponseMessage for pb::UseItemRsp {
    type Request = pb::UseItemReq;
}
impl xproto::RequestMessage for pb::MailListReq {
    type Response = pb::MailListRsp;
}
impl xproto::ResponseMessage for pb::MailListRsp {
    type Request = pb::MailListReq;
}
impl xproto::RequestMessage for pb::MailReadReq {
    type Response = pb::MailReadRsp;
}
impl xproto::ResponseMessage for pb::MailReadRsp {
    type Request = pb::MailReadReq;
}
impl xproto::RequestMessage for pb::MailDeleteReq {
    type Response = pb::MailDeleteRsp;
}
impl xproto::ResponseMessage for pb::MailDeleteRsp {
    type Request = pb::MailDeleteReq;
}
impl xproto::RequestMessage for pb::MailClaimReq {
    type Response = pb::MailClaimRsp;
}
impl xproto::ResponseMessage for pb::MailClaimRsp {
    type Request = pb::MailClaimReq;
}
impl xproto::NotificationMessage for pb::MailInfoNtf {}
impl xproto::RequestMessage for pb::AddItemsReq {
    type Response = pb::AddItemsRsp;
}
impl xproto::ResponseMessage for pb::AddItemsRsp {
    type Request = pb::AddItemsReq;
}
impl xproto::RequestMessage for pb::RemoveItemsReq {
    type Response = pb::RemoveItemsRsp;
}
impl xproto::ResponseMessage for pb::RemoveItemsRsp {
    type Request = pb::RemoveItemsReq;
}
impl xproto::RequestMessage for pb::CheckItemsReq {
    type Response = pb::CheckItemsRsp;
}
impl xproto::ResponseMessage for pb::CheckItemsRsp {
    type Request = pb::CheckItemsReq;
}
impl xproto::RequestMessage for pb::SendMailReq {
    type Response = pb::SendMailRsp;
}
impl xproto::ResponseMessage for pb::SendMailRsp {
    type Request = pb::SendMailReq;
}
impl xproto::NotificationMessage for pb::MailPushNtf {}
impl xproto::RequestMessage for pb::AuthLoginReq {
    type Response = pb::AuthLoginRsp;
}
impl xproto::ResponseMessage for pb::AuthLoginRsp {
    type Request = pb::AuthLoginReq;
}
impl xproto::RequestMessage for pb::AuthUseRoleReq {
    type Response = pb::AuthUseRoleRsp;
}
impl xproto::ResponseMessage for pb::AuthUseRoleRsp {
    type Request = pb::AuthUseRoleReq;
}
impl xproto::RequestMessage for pb::GamerInfoReq {
    type Response = pb::GamerInfoRsp;
}
impl xproto::ResponseMessage for pb::GamerInfoRsp {
    type Request = pb::GamerInfoReq;
}
impl xproto::RequestMessage for pb::ConfigKeyReq {
    type Response = pb::ConfigKeyRsp;
}
impl xproto::ResponseMessage for pb::ConfigKeyRsp {
    type Request = pb::ConfigKeyReq;
}
impl xproto::RequestMessage for pb::ConfigManifestReq {
    type Response = pb::ConfigManifestRsp;
}
impl xproto::ResponseMessage for pb::ConfigManifestRsp {
    type Request = pb::ConfigManifestReq;
}
impl xproto::NotificationMessage for pb::AckNtf {}
impl xproto::RequestMessage for pb::PingReq {
    type Response = pb::PingRsp;
}
impl xproto::ResponseMessage for pb::PingRsp {
    type Request = pb::PingReq;
}
impl xproto::RequestMessage for pb::LoginReq {
    type Response = pb::LoginRsp;
}
impl xproto::ResponseMessage for pb::LoginRsp {
    type Request = pb::LoginReq;
}
impl xproto::RequestMessage for pb::ReconnectReq {
    type Response = pb::ReconnectRsp;
}
impl xproto::ResponseMessage for pb::ReconnectRsp {
    type Request = pb::ReconnectReq;
}
impl xproto::RequestMessage for pb::LogoutReq {
    type Response = pb::LogoutRsp;
}
impl xproto::ResponseMessage for pb::LogoutRsp {
    type Request = pb::LogoutReq;
}
impl xproto::NotificationMessage for pb::KickNtf {}
impl xproto::RequestMessage for pb::LogicLoginReq {
    type Response = pb::LogicLoginRsp;
}
impl xproto::ResponseMessage for pb::LogicLoginRsp {
    type Request = pb::LogicLoginReq;
}
impl xproto::NotificationMessage for pb::LogicDisconnectNtf {}
impl xproto::RequestMessage for pb::KickSessionReq {
    type Response = pb::KickSessionRsp;
}
impl xproto::ResponseMessage for pb::KickSessionRsp {
    type Request = pb::KickSessionReq;
}

pub(crate) fn register_all_messages(registry: &mut xproto::MessageRegistry) -> Result<(), ProtocolError> {
    registry.register_application_descriptor(100, "xkk.v1.PlayerInfoReq")?;
    registry.register_application_descriptor(101, "xkk.v1.PlayerInfoRsp")?;
    registry.register_application_descriptor(102, "xkk.v1.UseItemReq")?;
    registry.register_application_descriptor(103, "xkk.v1.UseItemRsp")?;
    registry.register_application_descriptor(110, "xkk.v1.MailListReq")?;
    registry.register_application_descriptor(111, "xkk.v1.MailListRsp")?;
    registry.register_application_descriptor(112, "xkk.v1.MailReadReq")?;
    registry.register_application_descriptor(113, "xkk.v1.MailReadRsp")?;
    registry.register_application_descriptor(114, "xkk.v1.MailDeleteReq")?;
    registry.register_application_descriptor(115, "xkk.v1.MailDeleteRsp")?;
    registry.register_application_descriptor(116, "xkk.v1.MailClaimReq")?;
    registry.register_application_descriptor(117, "xkk.v1.MailClaimRsp")?;
    registry.register_application_descriptor(118, "xkk.v1.MailInfoNtf")?;
    registry.register_application_descriptor(1000, "xkk.v1.AddItemsReq")?;
    registry.register_application_descriptor(1001, "xkk.v1.AddItemsRsp")?;
    registry.register_application_descriptor(1002, "xkk.v1.RemoveItemsReq")?;
    registry.register_application_descriptor(1003, "xkk.v1.RemoveItemsRsp")?;
    registry.register_application_descriptor(1004, "xkk.v1.CheckItemsReq")?;
    registry.register_application_descriptor(1005, "xkk.v1.CheckItemsRsp")?;
    registry.register_application_descriptor(1010, "xkk.v1.SendMailReq")?;
    registry.register_application_descriptor(1011, "xkk.v1.SendMailRsp")?;
    registry.register_application_descriptor(1012, "xkk.v1.MailPushNtf")?;
    registry.register_application_descriptor(2000, "xkk.v1.AuthLoginReq")?;
    registry.register_application_descriptor(2001, "xkk.v1.AuthLoginRsp")?;
    registry.register_application_descriptor(2002, "xkk.v1.AuthUseRoleReq")?;
    registry.register_application_descriptor(2003, "xkk.v1.AuthUseRoleRsp")?;
    registry.register_application_descriptor(2100, "xkk.v1.GamerInfoReq")?;
    registry.register_application_descriptor(2101, "xkk.v1.GamerInfoRsp")?;
    registry.register_application_descriptor(2102, "xkk.v1.ConfigKeyReq")?;
    registry.register_application_descriptor(2103, "xkk.v1.ConfigKeyRsp")?;
    registry.register_application_descriptor(2104, "xkk.v1.ConfigManifestReq")?;
    registry.register_application_descriptor(2105, "xkk.v1.ConfigManifestRsp")?;
    registry.register_application_descriptor(3000, "xkk.v1.AckNtf")?;
    registry.register_application_descriptor(3001, "xkk.v1.PingReq")?;
    registry.register_application_descriptor(3002, "xkk.v1.PingRsp")?;
    registry.register_application_descriptor(3003, "xkk.v1.LoginReq")?;
    registry.register_application_descriptor(3004, "xkk.v1.LoginRsp")?;
    registry.register_application_descriptor(3005, "xkk.v1.ReconnectReq")?;
    registry.register_application_descriptor(3006, "xkk.v1.ReconnectRsp")?;
    registry.register_application_descriptor(3007, "xkk.v1.LogoutReq")?;
    registry.register_application_descriptor(3008, "xkk.v1.LogoutRsp")?;
    registry.register_application_descriptor(3009, "xkk.v1.KickNtf")?;
    registry.register_application_descriptor(3010, "xkk.v1.LogicLoginReq")?;
    registry.register_application_descriptor(3011, "xkk.v1.LogicLoginRsp")?;
    registry.register_application_descriptor(3012, "xkk.v1.LogicDisconnectNtf")?;
    registry.register_application_descriptor(3013, "xkk.v1.KickSessionReq")?;
    registry.register_application_descriptor(3014, "xkk.v1.KickSessionRsp")?;
    Ok(())
}

pub(crate) fn response_id(msgid: crate::pb::MsgId) -> Option<crate::pb::MsgId> {
    match msgid {
        crate::pb::MsgId::PlayerInfoReq => Some(crate::pb::MsgId::PlayerInfoRsp),
        crate::pb::MsgId::UseItemReq => Some(crate::pb::MsgId::UseItemRsp),
        crate::pb::MsgId::MailListReq => Some(crate::pb::MsgId::MailListRsp),
        crate::pb::MsgId::MailReadReq => Some(crate::pb::MsgId::MailReadRsp),
        crate::pb::MsgId::MailDeleteReq => Some(crate::pb::MsgId::MailDeleteRsp),
        crate::pb::MsgId::MailClaimReq => Some(crate::pb::MsgId::MailClaimRsp),
        crate::pb::MsgId::AddItemsReq => Some(crate::pb::MsgId::AddItemsRsp),
        crate::pb::MsgId::RemoveItemsReq => Some(crate::pb::MsgId::RemoveItemsRsp),
        crate::pb::MsgId::CheckItemsReq => Some(crate::pb::MsgId::CheckItemsRsp),
        crate::pb::MsgId::SendMailReq => Some(crate::pb::MsgId::SendMailRsp),
        crate::pb::MsgId::AuthLoginReq => Some(crate::pb::MsgId::AuthLoginRsp),
        crate::pb::MsgId::AuthUseRoleReq => Some(crate::pb::MsgId::AuthUseRoleRsp),
        crate::pb::MsgId::GamerInfoReq => Some(crate::pb::MsgId::GamerInfoRsp),
        crate::pb::MsgId::ConfigKeyReq => Some(crate::pb::MsgId::ConfigKeyRsp),
        crate::pb::MsgId::ConfigManifestReq => Some(crate::pb::MsgId::ConfigManifestRsp),
        crate::pb::MsgId::PingReq => Some(crate::pb::MsgId::PingRsp),
        crate::pb::MsgId::LoginReq => Some(crate::pb::MsgId::LoginRsp),
        crate::pb::MsgId::ReconnectReq => Some(crate::pb::MsgId::ReconnectRsp),
        crate::pb::MsgId::LogoutReq => Some(crate::pb::MsgId::LogoutRsp),
        crate::pb::MsgId::LogicLoginReq => Some(crate::pb::MsgId::LogicLoginRsp),
        crate::pb::MsgId::KickSessionReq => Some(crate::pb::MsgId::KickSessionRsp),
        _ => None,
    }
}

pub(crate) fn request_id(msgid: crate::pb::MsgId) -> Option<crate::pb::MsgId> {
    match msgid {
        crate::pb::MsgId::PlayerInfoRsp => Some(crate::pb::MsgId::PlayerInfoReq),
        crate::pb::MsgId::UseItemRsp => Some(crate::pb::MsgId::UseItemReq),
        crate::pb::MsgId::MailListRsp => Some(crate::pb::MsgId::MailListReq),
        crate::pb::MsgId::MailReadRsp => Some(crate::pb::MsgId::MailReadReq),
        crate::pb::MsgId::MailDeleteRsp => Some(crate::pb::MsgId::MailDeleteReq),
        crate::pb::MsgId::MailClaimRsp => Some(crate::pb::MsgId::MailClaimReq),
        crate::pb::MsgId::AddItemsRsp => Some(crate::pb::MsgId::AddItemsReq),
        crate::pb::MsgId::RemoveItemsRsp => Some(crate::pb::MsgId::RemoveItemsReq),
        crate::pb::MsgId::CheckItemsRsp => Some(crate::pb::MsgId::CheckItemsReq),
        crate::pb::MsgId::SendMailRsp => Some(crate::pb::MsgId::SendMailReq),
        crate::pb::MsgId::AuthLoginRsp => Some(crate::pb::MsgId::AuthLoginReq),
        crate::pb::MsgId::AuthUseRoleRsp => Some(crate::pb::MsgId::AuthUseRoleReq),
        crate::pb::MsgId::GamerInfoRsp => Some(crate::pb::MsgId::GamerInfoReq),
        crate::pb::MsgId::ConfigKeyRsp => Some(crate::pb::MsgId::ConfigKeyReq),
        crate::pb::MsgId::ConfigManifestRsp => Some(crate::pb::MsgId::ConfigManifestReq),
        crate::pb::MsgId::PingRsp => Some(crate::pb::MsgId::PingReq),
        crate::pb::MsgId::LoginRsp => Some(crate::pb::MsgId::LoginReq),
        crate::pb::MsgId::ReconnectRsp => Some(crate::pb::MsgId::ReconnectReq),
        crate::pb::MsgId::LogoutRsp => Some(crate::pb::MsgId::LogoutReq),
        crate::pb::MsgId::LogicLoginRsp => Some(crate::pb::MsgId::LogicLoginReq),
        crate::pb::MsgId::KickSessionRsp => Some(crate::pb::MsgId::KickSessionReq),
        _ => None,
    }
}

pub(crate) fn message_id_for_type(
    type_id: std::any::TypeId,
) -> Option<crate::pb::MsgId> {
    if type_id == std::any::TypeId::of::<pb::PlayerInfoReq>() {
        return Some(crate::pb::MsgId::PlayerInfoReq);
    }
    if type_id == std::any::TypeId::of::<pb::PlayerInfoRsp>() {
        return Some(crate::pb::MsgId::PlayerInfoRsp);
    }
    if type_id == std::any::TypeId::of::<pb::UseItemReq>() {
        return Some(crate::pb::MsgId::UseItemReq);
    }
    if type_id == std::any::TypeId::of::<pb::UseItemRsp>() {
        return Some(crate::pb::MsgId::UseItemRsp);
    }
    if type_id == std::any::TypeId::of::<pb::MailListReq>() {
        return Some(crate::pb::MsgId::MailListReq);
    }
    if type_id == std::any::TypeId::of::<pb::MailListRsp>() {
        return Some(crate::pb::MsgId::MailListRsp);
    }
    if type_id == std::any::TypeId::of::<pb::MailReadReq>() {
        return Some(crate::pb::MsgId::MailReadReq);
    }
    if type_id == std::any::TypeId::of::<pb::MailReadRsp>() {
        return Some(crate::pb::MsgId::MailReadRsp);
    }
    if type_id == std::any::TypeId::of::<pb::MailDeleteReq>() {
        return Some(crate::pb::MsgId::MailDeleteReq);
    }
    if type_id == std::any::TypeId::of::<pb::MailDeleteRsp>() {
        return Some(crate::pb::MsgId::MailDeleteRsp);
    }
    if type_id == std::any::TypeId::of::<pb::MailClaimReq>() {
        return Some(crate::pb::MsgId::MailClaimReq);
    }
    if type_id == std::any::TypeId::of::<pb::MailClaimRsp>() {
        return Some(crate::pb::MsgId::MailClaimRsp);
    }
    if type_id == std::any::TypeId::of::<pb::MailInfoNtf>() {
        return Some(crate::pb::MsgId::MailInfoNtf);
    }
    if type_id == std::any::TypeId::of::<pb::AddItemsReq>() {
        return Some(crate::pb::MsgId::AddItemsReq);
    }
    if type_id == std::any::TypeId::of::<pb::AddItemsRsp>() {
        return Some(crate::pb::MsgId::AddItemsRsp);
    }
    if type_id == std::any::TypeId::of::<pb::RemoveItemsReq>() {
        return Some(crate::pb::MsgId::RemoveItemsReq);
    }
    if type_id == std::any::TypeId::of::<pb::RemoveItemsRsp>() {
        return Some(crate::pb::MsgId::RemoveItemsRsp);
    }
    if type_id == std::any::TypeId::of::<pb::CheckItemsReq>() {
        return Some(crate::pb::MsgId::CheckItemsReq);
    }
    if type_id == std::any::TypeId::of::<pb::CheckItemsRsp>() {
        return Some(crate::pb::MsgId::CheckItemsRsp);
    }
    if type_id == std::any::TypeId::of::<pb::SendMailReq>() {
        return Some(crate::pb::MsgId::SendMailReq);
    }
    if type_id == std::any::TypeId::of::<pb::SendMailRsp>() {
        return Some(crate::pb::MsgId::SendMailRsp);
    }
    if type_id == std::any::TypeId::of::<pb::MailPushNtf>() {
        return Some(crate::pb::MsgId::MailPushNtf);
    }
    if type_id == std::any::TypeId::of::<pb::AuthLoginReq>() {
        return Some(crate::pb::MsgId::AuthLoginReq);
    }
    if type_id == std::any::TypeId::of::<pb::AuthLoginRsp>() {
        return Some(crate::pb::MsgId::AuthLoginRsp);
    }
    if type_id == std::any::TypeId::of::<pb::AuthUseRoleReq>() {
        return Some(crate::pb::MsgId::AuthUseRoleReq);
    }
    if type_id == std::any::TypeId::of::<pb::AuthUseRoleRsp>() {
        return Some(crate::pb::MsgId::AuthUseRoleRsp);
    }
    if type_id == std::any::TypeId::of::<pb::GamerInfoReq>() {
        return Some(crate::pb::MsgId::GamerInfoReq);
    }
    if type_id == std::any::TypeId::of::<pb::GamerInfoRsp>() {
        return Some(crate::pb::MsgId::GamerInfoRsp);
    }
    if type_id == std::any::TypeId::of::<pb::ConfigKeyReq>() {
        return Some(crate::pb::MsgId::ConfigKeyReq);
    }
    if type_id == std::any::TypeId::of::<pb::ConfigKeyRsp>() {
        return Some(crate::pb::MsgId::ConfigKeyRsp);
    }
    if type_id == std::any::TypeId::of::<pb::ConfigManifestReq>() {
        return Some(crate::pb::MsgId::ConfigManifestReq);
    }
    if type_id == std::any::TypeId::of::<pb::ConfigManifestRsp>() {
        return Some(crate::pb::MsgId::ConfigManifestRsp);
    }
    if type_id == std::any::TypeId::of::<pb::AckNtf>() {
        return Some(crate::pb::MsgId::AckNtf);
    }
    if type_id == std::any::TypeId::of::<pb::PingReq>() {
        return Some(crate::pb::MsgId::PingReq);
    }
    if type_id == std::any::TypeId::of::<pb::PingRsp>() {
        return Some(crate::pb::MsgId::PingRsp);
    }
    if type_id == std::any::TypeId::of::<pb::LoginReq>() {
        return Some(crate::pb::MsgId::LoginReq);
    }
    if type_id == std::any::TypeId::of::<pb::LoginRsp>() {
        return Some(crate::pb::MsgId::LoginRsp);
    }
    if type_id == std::any::TypeId::of::<pb::ReconnectReq>() {
        return Some(crate::pb::MsgId::ReconnectReq);
    }
    if type_id == std::any::TypeId::of::<pb::ReconnectRsp>() {
        return Some(crate::pb::MsgId::ReconnectRsp);
    }
    if type_id == std::any::TypeId::of::<pb::LogoutReq>() {
        return Some(crate::pb::MsgId::LogoutReq);
    }
    if type_id == std::any::TypeId::of::<pb::LogoutRsp>() {
        return Some(crate::pb::MsgId::LogoutRsp);
    }
    if type_id == std::any::TypeId::of::<pb::KickNtf>() {
        return Some(crate::pb::MsgId::KickNtf);
    }
    if type_id == std::any::TypeId::of::<pb::LogicLoginReq>() {
        return Some(crate::pb::MsgId::LogicLoginReq);
    }
    if type_id == std::any::TypeId::of::<pb::LogicLoginRsp>() {
        return Some(crate::pb::MsgId::LogicLoginRsp);
    }
    if type_id == std::any::TypeId::of::<pb::LogicDisconnectNtf>() {
        return Some(crate::pb::MsgId::LogicDisconnectNtf);
    }
    if type_id == std::any::TypeId::of::<pb::KickSessionReq>() {
        return Some(crate::pb::MsgId::KickSessionReq);
    }
    if type_id == std::any::TypeId::of::<pb::KickSessionRsp>() {
        return Some(crate::pb::MsgId::KickSessionRsp);
    }
    None
}

pub(crate) fn new_message_by_id(
    msgid: crate::pb::MsgId,
) -> Option<Box<dyn std::any::Any + Send + Sync>> {
    match msgid {
        crate::pb::MsgId::PlayerInfoReq => Some(Box::new(pb::PlayerInfoReq::default())),
        crate::pb::MsgId::PlayerInfoRsp => Some(Box::new(pb::PlayerInfoRsp::default())),
        crate::pb::MsgId::UseItemReq => Some(Box::new(pb::UseItemReq::default())),
        crate::pb::MsgId::UseItemRsp => Some(Box::new(pb::UseItemRsp::default())),
        crate::pb::MsgId::MailListReq => Some(Box::new(pb::MailListReq::default())),
        crate::pb::MsgId::MailListRsp => Some(Box::new(pb::MailListRsp::default())),
        crate::pb::MsgId::MailReadReq => Some(Box::new(pb::MailReadReq::default())),
        crate::pb::MsgId::MailReadRsp => Some(Box::new(pb::MailReadRsp::default())),
        crate::pb::MsgId::MailDeleteReq => Some(Box::new(pb::MailDeleteReq::default())),
        crate::pb::MsgId::MailDeleteRsp => Some(Box::new(pb::MailDeleteRsp::default())),
        crate::pb::MsgId::MailClaimReq => Some(Box::new(pb::MailClaimReq::default())),
        crate::pb::MsgId::MailClaimRsp => Some(Box::new(pb::MailClaimRsp::default())),
        crate::pb::MsgId::MailInfoNtf => Some(Box::new(pb::MailInfoNtf::default())),
        crate::pb::MsgId::AddItemsReq => Some(Box::new(pb::AddItemsReq::default())),
        crate::pb::MsgId::AddItemsRsp => Some(Box::new(pb::AddItemsRsp::default())),
        crate::pb::MsgId::RemoveItemsReq => Some(Box::new(pb::RemoveItemsReq::default())),
        crate::pb::MsgId::RemoveItemsRsp => Some(Box::new(pb::RemoveItemsRsp::default())),
        crate::pb::MsgId::CheckItemsReq => Some(Box::new(pb::CheckItemsReq::default())),
        crate::pb::MsgId::CheckItemsRsp => Some(Box::new(pb::CheckItemsRsp::default())),
        crate::pb::MsgId::SendMailReq => Some(Box::new(pb::SendMailReq::default())),
        crate::pb::MsgId::SendMailRsp => Some(Box::new(pb::SendMailRsp::default())),
        crate::pb::MsgId::MailPushNtf => Some(Box::new(pb::MailPushNtf::default())),
        crate::pb::MsgId::AuthLoginReq => Some(Box::new(pb::AuthLoginReq::default())),
        crate::pb::MsgId::AuthLoginRsp => Some(Box::new(pb::AuthLoginRsp::default())),
        crate::pb::MsgId::AuthUseRoleReq => Some(Box::new(pb::AuthUseRoleReq::default())),
        crate::pb::MsgId::AuthUseRoleRsp => Some(Box::new(pb::AuthUseRoleRsp::default())),
        crate::pb::MsgId::GamerInfoReq => Some(Box::new(pb::GamerInfoReq::default())),
        crate::pb::MsgId::GamerInfoRsp => Some(Box::new(pb::GamerInfoRsp::default())),
        crate::pb::MsgId::ConfigKeyReq => Some(Box::new(pb::ConfigKeyReq::default())),
        crate::pb::MsgId::ConfigKeyRsp => Some(Box::new(pb::ConfigKeyRsp::default())),
        crate::pb::MsgId::ConfigManifestReq => Some(Box::new(pb::ConfigManifestReq::default())),
        crate::pb::MsgId::ConfigManifestRsp => Some(Box::new(pb::ConfigManifestRsp::default())),
        crate::pb::MsgId::AckNtf => Some(Box::new(pb::AckNtf::default())),
        crate::pb::MsgId::PingReq => Some(Box::new(pb::PingReq::default())),
        crate::pb::MsgId::PingRsp => Some(Box::new(pb::PingRsp::default())),
        crate::pb::MsgId::LoginReq => Some(Box::new(pb::LoginReq::default())),
        crate::pb::MsgId::LoginRsp => Some(Box::new(pb::LoginRsp::default())),
        crate::pb::MsgId::ReconnectReq => Some(Box::new(pb::ReconnectReq::default())),
        crate::pb::MsgId::ReconnectRsp => Some(Box::new(pb::ReconnectRsp::default())),
        crate::pb::MsgId::LogoutReq => Some(Box::new(pb::LogoutReq::default())),
        crate::pb::MsgId::LogoutRsp => Some(Box::new(pb::LogoutRsp::default())),
        crate::pb::MsgId::KickNtf => Some(Box::new(pb::KickNtf::default())),
        crate::pb::MsgId::LogicLoginReq => Some(Box::new(pb::LogicLoginReq::default())),
        crate::pb::MsgId::LogicLoginRsp => Some(Box::new(pb::LogicLoginRsp::default())),
        crate::pb::MsgId::LogicDisconnectNtf => Some(Box::new(pb::LogicDisconnectNtf::default())),
        crate::pb::MsgId::KickSessionReq => Some(Box::new(pb::KickSessionReq::default())),
        crate::pb::MsgId::KickSessionRsp => Some(Box::new(pb::KickSessionRsp::default())),
        _ => None,
    }
}
