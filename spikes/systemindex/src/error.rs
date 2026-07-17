use std::fmt;

use serde::Serialize;

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ErrorKind {
    InvalidInput,
    NotRunnable,
    VerificationFailed,
}

#[derive(Debug)]
pub struct SpikeError {
    kind: ErrorKind,
    message: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ErrorEvidence<'a> {
    pub kind: ErrorKind,
    pub message: &'a str,
}

impl SpikeError {
    pub fn invalid_input(message: impl Into<String>) -> Self {
        Self {
            kind: ErrorKind::InvalidInput,
            message: message.into(),
        }
    }

    pub fn not_runnable(message: impl Into<String>) -> Self {
        Self {
            kind: ErrorKind::NotRunnable,
            message: message.into(),
        }
    }

    pub fn verification_failed(message: impl Into<String>) -> Self {
        Self {
            kind: ErrorKind::VerificationFailed,
            message: message.into(),
        }
    }

    pub fn exit_code(&self) -> i32 {
        match self.kind {
            ErrorKind::InvalidInput | ErrorKind::VerificationFailed => 1,
            ErrorKind::NotRunnable => 2,
        }
    }

    pub fn evidence(&self) -> ErrorEvidence<'_> {
        ErrorEvidence {
            kind: self.kind,
            message: &self.message,
        }
    }
}

impl fmt::Display for SpikeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for SpikeError {}
