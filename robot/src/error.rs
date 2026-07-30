use std::{error::Error, fmt};

pub(crate) type Result<T> = std::result::Result<T, RobotError>;

#[derive(Debug)]
pub struct RobotError(String);

impl fmt::Display for RobotError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl Error for RobotError {}

pub(crate) fn failure(message: impl Into<String>) -> RobotError {
    RobotError(message.into())
}
