use std::fmt;

use serde::Serialize;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OperationCounters {
    pub search_folder_factory_created: u32,
    pub scope_set: u32,
    pub search_folder_enumerated: u32,
}

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
    counters: OperationCounters,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ErrorEvidence<'a> {
    pub kind: ErrorKind,
    pub message: &'a str,
    pub counters: OperationCounters,
}

impl SpikeError {
    pub fn invalid_input(message: impl Into<String>) -> Self {
        Self {
            kind: ErrorKind::InvalidInput,
            message: message.into(),
            counters: OperationCounters::default(),
        }
    }

    pub fn not_runnable(message: impl Into<String>) -> Self {
        Self {
            kind: ErrorKind::NotRunnable,
            message: message.into(),
            counters: OperationCounters::default(),
        }
    }

    pub fn verification_failed(message: impl Into<String>) -> Self {
        Self {
            kind: ErrorKind::VerificationFailed,
            message: message.into(),
            counters: OperationCounters::default(),
        }
    }

    pub fn exit_code(&self) -> i32 {
        match self.kind {
            ErrorKind::InvalidInput | ErrorKind::VerificationFailed => 1,
            ErrorKind::NotRunnable => 2,
        }
    }

    pub fn with_counters(mut self, counters: OperationCounters) -> Self {
        self.counters = counters;
        self
    }

    pub fn evidence(&self) -> ErrorEvidence<'_> {
        ErrorEvidence {
            kind: self.kind,
            message: &self.message,
            counters: self.counters,
        }
    }
}

impl fmt::Display for SpikeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for SpikeError {}
