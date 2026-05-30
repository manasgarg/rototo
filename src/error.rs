use std::error::Error;
use std::fmt::{self, Display};

pub type Result<T> = std::result::Result<T, RototoError>;

#[derive(Debug)]
pub struct RototoError {
    message: String,
}

impl RototoError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl Display for RototoError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.message.fmt(f)
    }
}

impl Error for RototoError {}
