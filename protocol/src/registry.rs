use thiserror::Error;
use xproto::MessageRegistry;

use crate::pb;

include!(concat!(env!("OUT_DIR"), "/xkk.registry.rs"));

#[derive(Debug, Error)]
pub enum ProtocolError {
    #[error(transparent)]
    Registry(#[from] xproto::Error),
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
