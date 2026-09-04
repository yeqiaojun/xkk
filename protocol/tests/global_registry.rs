use xkk_protocol::{MsgId, init_global_registry, pb, response_for};
use xproto::{Error, MessageRegistry, global_registry};

#[test]
fn global_registry_is_complete_and_initialized_once() {
    let err = xproto::init_global_registry(MessageRegistry::new()).unwrap_err();
    assert!(matches!(err, Error::IncompleteRegistry { .. }));
    assert!(matches!(global_registry(), Err(Error::RegistryNotInitialized)));

    init_global_registry().unwrap();
    let registry = global_registry().unwrap();
    assert_eq!(registry.response_id(MsgId::PingReq.as_u16()), Some(MsgId::PingRsp.as_u16()));
    assert_eq!(registry.request_id(MsgId::PingRsp.as_u16()), Some(MsgId::PingReq.as_u16()));
    assert_eq!(response_for(MsgId::PingReq), Some(MsgId::PingRsp));
    assert_eq!(registry.message_id_for_type::<pb::PingReq>(), Some(MsgId::PingReq.as_u16()));
    assert!(registry.new_message(MsgId::PingReq.as_u16()).unwrap().downcast_ref::<pb::PingReq>().is_some());

    let err = init_global_registry().unwrap_err();
    assert!(matches!(err, xkk_protocol::ProtocolError::Registry(Error::RegistryAlreadyInitialized)));
}
