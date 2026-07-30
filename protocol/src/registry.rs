use std::sync::Arc;

use thiserror::Error;
use xframe::xproto::registry::MessageRegistry;

use crate::{MsgId, pb, response_for};

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
