mod message;
mod registry;
mod status;

pub mod pb {
    include!(concat!(env!("OUT_DIR"), "/xkk.v1.rs"));
    include!(concat!(env!("OUT_DIR"), "/xkk.xmongo.rs"));
    include!(concat!(env!("OUT_DIR"), "/model.xmongo.rs"));
}

pub use message::{
    MessageKind, OUTBOX_EXCLUDED_RANGES, RouteTarget, from_u16, is_outbox_message, response_for,
    route_target,
};
pub use pb::MsgId;
pub use registry::{ProtocolError, client_registry, validate_pair};
pub use status::{code, error_status, ok_status};

#[cfg(test)]
mod tests;
