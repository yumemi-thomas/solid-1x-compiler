//! The compiler's own error type.
//!
//! Nothing here refers to Oxc or to a host adapter, so the compile interface
//! stays usable from plain Rust — the Node-API `Error` is a detail of
//! `node_adapter`, not of compilation.

use std::fmt;

/// The stage at which compilation failed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CompileErrorKind {
    Parse,
    Configuration,
    Transform,
}

/// An owned compiler error that does not expose Oxc or host-adapter types.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompileError {
    kind: CompileErrorKind,
    message: String,
}

impl CompileError {
    #[must_use]
    pub fn parse(message: impl Into<String>) -> Self {
        Self::new(CompileErrorKind::Parse, message)
    }

    #[must_use]
    pub fn configuration(message: impl Into<String>) -> Self {
        Self::new(CompileErrorKind::Configuration, message)
    }

    #[must_use]
    pub fn transform(message: impl Into<String>) -> Self {
        Self::new(CompileErrorKind::Transform, message)
    }

    /// Compatibility constructor used by transform internals, which raise
    /// unsupported-construct and validation failures by reason string.
    #[must_use]
    pub(crate) fn from_reason(message: impl Into<String>) -> Self {
        Self::transform(message)
    }

    #[must_use]
    pub fn kind(&self) -> CompileErrorKind {
        self.kind
    }

    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }

    fn new(kind: CompileErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }
}

impl fmt::Display for CompileError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for CompileError {}

pub(crate) type Error = CompileError;
pub(crate) type Result<T> = std::result::Result<T, CompileError>;
