use crate::pb;

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
    pb::Status { code: code::OK, message: String::new() }
}

pub fn error_status(code: i32, message: impl Into<String>) -> pb::Status {
    pb::Status { code, message: message.into() }
}
