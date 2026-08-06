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
    registry.register_application_message::<pb::PlayerInfoReq>("xkk.v1.PlayerInfoReq")?;
    registry.register_application_message::<pb::PlayerInfoRsp>("xkk.v1.PlayerInfoRsp")?;
    registry.register_application_message::<pb::UseItemReq>("xkk.v1.UseItemReq")?;
    registry.register_application_message::<pb::UseItemRsp>("xkk.v1.UseItemRsp")?;
    registry.register_application_message::<pb::MailListReq>("xkk.v1.MailListReq")?;
    registry.register_application_message::<pb::MailListRsp>("xkk.v1.MailListRsp")?;
    registry.register_application_message::<pb::MailReadReq>("xkk.v1.MailReadReq")?;
    registry.register_application_message::<pb::MailReadRsp>("xkk.v1.MailReadRsp")?;
    registry.register_application_message::<pb::MailDeleteReq>("xkk.v1.MailDeleteReq")?;
    registry.register_application_message::<pb::MailDeleteRsp>("xkk.v1.MailDeleteRsp")?;
    registry.register_application_message::<pb::MailClaimReq>("xkk.v1.MailClaimReq")?;
    registry.register_application_message::<pb::MailClaimRsp>("xkk.v1.MailClaimRsp")?;
    registry.register_application_message::<pb::MailInfoNtf>("xkk.v1.MailInfoNtf")?;
    registry.register_application_message::<pb::AddItemsReq>("xkk.v1.AddItemsReq")?;
    registry.register_application_message::<pb::AddItemsRsp>("xkk.v1.AddItemsRsp")?;
    registry.register_application_message::<pb::RemoveItemsReq>("xkk.v1.RemoveItemsReq")?;
    registry.register_application_message::<pb::RemoveItemsRsp>("xkk.v1.RemoveItemsRsp")?;
    registry.register_application_message::<pb::CheckItemsReq>("xkk.v1.CheckItemsReq")?;
    registry.register_application_message::<pb::CheckItemsRsp>("xkk.v1.CheckItemsRsp")?;
    registry.register_application_message::<pb::SendMailReq>("xkk.v1.SendMailReq")?;
    registry.register_application_message::<pb::SendMailRsp>("xkk.v1.SendMailRsp")?;
    registry.register_application_message::<pb::MailPushNtf>("xkk.v1.MailPushNtf")?;
    registry.register_application_message::<pb::AuthLoginReq>("xkk.v1.AuthLoginReq")?;
    registry.register_application_message::<pb::AuthLoginRsp>("xkk.v1.AuthLoginRsp")?;
    registry.register_application_message::<pb::AuthUseRoleReq>("xkk.v1.AuthUseRoleReq")?;
    registry.register_application_message::<pb::AuthUseRoleRsp>("xkk.v1.AuthUseRoleRsp")?;
    registry.register_application_message::<pb::GamerInfoReq>("xkk.v1.GamerInfoReq")?;
    registry.register_application_message::<pb::GamerInfoRsp>("xkk.v1.GamerInfoRsp")?;
    registry.register_application_message::<pb::AckNtf>("xkk.v1.AckNtf")?;
    registry.register_application_message::<pb::PingReq>("xkk.v1.PingReq")?;
    registry.register_application_message::<pb::PingRsp>("xkk.v1.PingRsp")?;
    registry.register_application_message::<pb::LoginReq>("xkk.v1.LoginReq")?;
    registry.register_application_message::<pb::LoginRsp>("xkk.v1.LoginRsp")?;
    registry.register_application_message::<pb::ReconnectReq>("xkk.v1.ReconnectReq")?;
    registry.register_application_message::<pb::ReconnectRsp>("xkk.v1.ReconnectRsp")?;
    registry.register_application_message::<pb::LogoutReq>("xkk.v1.LogoutReq")?;
    registry.register_application_message::<pb::LogoutRsp>("xkk.v1.LogoutRsp")?;
    registry.register_application_message::<pb::KickNtf>("xkk.v1.KickNtf")?;
    registry.register_application_message::<pb::LogicLoginReq>("xkk.v1.LogicLoginReq")?;
    registry.register_application_message::<pb::LogicLoginRsp>("xkk.v1.LogicLoginRsp")?;
    registry.register_application_message::<pb::LogicDisconnectNtf>("xkk.v1.LogicDisconnectNtf")?;
    registry.register_application_message::<pb::KickSessionReq>("xkk.v1.KickSessionReq")?;
    registry.register_application_message::<pb::KickSessionRsp>("xkk.v1.KickSessionRsp")?;
    registry.register_request_response::<pb::PlayerInfoReq, pb::PlayerInfoRsp>()?;
    registry.register_request_response::<pb::UseItemReq, pb::UseItemRsp>()?;
    registry.register_request_response::<pb::MailListReq, pb::MailListRsp>()?;
    registry.register_request_response::<pb::MailReadReq, pb::MailReadRsp>()?;
    registry.register_request_response::<pb::MailDeleteReq, pb::MailDeleteRsp>()?;
    registry.register_request_response::<pb::MailClaimReq, pb::MailClaimRsp>()?;
    registry.register_notification::<pb::MailInfoNtf>()?;
    registry.register_request_response::<pb::AddItemsReq, pb::AddItemsRsp>()?;
    registry.register_request_response::<pb::RemoveItemsReq, pb::RemoveItemsRsp>()?;
    registry.register_request_response::<pb::CheckItemsReq, pb::CheckItemsRsp>()?;
    registry.register_request_response::<pb::SendMailReq, pb::SendMailRsp>()?;
    registry.register_notification::<pb::MailPushNtf>()?;
    registry.register_request_response::<pb::AuthLoginReq, pb::AuthLoginRsp>()?;
    registry.register_request_response::<pb::AuthUseRoleReq, pb::AuthUseRoleRsp>()?;
    registry.register_request_response::<pb::GamerInfoReq, pb::GamerInfoRsp>()?;
    registry.register_notification::<pb::AckNtf>()?;
    registry.register_request_response::<pb::PingReq, pb::PingRsp>()?;
    registry.register_request_response::<pb::LoginReq, pb::LoginRsp>()?;
    registry.register_request_response::<pb::ReconnectReq, pb::ReconnectRsp>()?;
    registry.register_request_response::<pb::LogoutReq, pb::LogoutRsp>()?;
    registry.register_notification::<pb::KickNtf>()?;
    registry.register_request_response::<pb::LogicLoginReq, pb::LogicLoginRsp>()?;
    registry.register_notification::<pb::LogicDisconnectNtf>()?;
    registry.register_request_response::<pb::KickSessionReq, pb::KickSessionRsp>()?;
    Ok(())
}
