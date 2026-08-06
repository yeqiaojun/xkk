mod message;
mod registry;
mod status;

pub mod pb {
    include!(concat!(env!("OUT_DIR"), "/xkk.v1.rs"));
    include!(concat!(env!("OUT_DIR"), "/xkk.xmongo.rs"));
    include!(concat!(env!("OUT_DIR"), "/model.xmongo.rs"));
}

pub use message::{
    MessageKind, RouteTarget, from_u16, is_outbox_message, response_for, route_target,
};
pub use pb::MsgId;
pub use registry::{ProtocolError, descriptor_set, init_global_registry, message_registry};
pub use status::{code, error_status, ok_status};

#[cfg(test)]
mod tests;
