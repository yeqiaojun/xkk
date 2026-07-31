use std::{any::Any, sync::Arc};

use thiserror::Error;
use xproto::MessageRegistry;

use crate::{MsgId, pb, response_for};

include!(concat!(env!("OUT_DIR"), "/xkk.registry.rs"));

#[derive(Debug, Error)]
pub enum ProtocolError {
    #[error(transparent)]
    Registry(#[from] xproto::Error),
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
    Ok(Arc::new(message_registry()?))
}

/// The checked-in descriptor set for all XKK wire and persistence messages.
pub fn descriptor_set() -> &'static [u8] {
    include_bytes!("../generated/xkk.descriptor.bin")
}

/// Builds the complete process registry: shared xproto controls followed by XKK messages.
pub fn message_registry() -> Result<MessageRegistry, ProtocolError> {
    let mut registry = MessageRegistry::from_descriptor_sets(&[
        xproto::control::descriptor_set(),
        descriptor_set(),
    ])?;
    xproto::control::register_control_messages(&mut registry)?;
    register_all_messages(&mut registry)?;
    Ok(registry)
}

/// Publishes the complete immutable registry before xframe admits network traffic.
pub fn init_global_registry() -> Result<(), ProtocolError> {
    xproto::init_global_registry(message_registry()?)?;
    Ok(())
}

/// Returns the protocol ID associated with a generated protobuf type.
pub fn message_id<T>() -> Option<MsgId>
where
    T: Any + 'static,
{
    message_id_for_type(std::any::TypeId::of::<T>())
}

/// Returns the protocol ID associated with a type-erased generated protobuf value.
pub fn message_id_of(message: &(dyn Any + Send + Sync)) -> Option<MsgId> {
    message_id_for_type(message.type_id())
}

/// Creates a default generated protobuf value for a protocol ID.
///
/// Downcast the returned value to the expected `pb` type.
pub fn new_message(msgid: MsgId) -> Option<Box<dyn Any + Send + Sync>> {
    new_message_by_id(msgid)
}
