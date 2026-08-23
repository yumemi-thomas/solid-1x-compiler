//! A Solid 1.x JSX compiler implemented with Oxc.
//!
//! The reusable core is [`compile`], expressed in owned Rust types. Setting
//! [`CompileOptions::semantic_trace`] additionally returns a [`SemanticTrace`]
//! describing how the emitted code will execute, recorded by the same lowering
//! pass. The Node addon is one adapter over that core and is compiled only
//! with the default `node` feature; building with `--no-default-features`
//! drops the Node-API dependency entirely.

mod compiler;
#[cfg(feature = "node")]
mod config;
#[cfg(feature = "node")]
mod directives;
mod dom;
mod error;
#[cfg(feature = "node")]
mod lazy;
#[cfg(feature = "node")]
mod node_adapter;
#[cfg(feature = "node")]
mod refresh;
mod semantic_trace;
mod shared;
mod ssr;
mod universal;

pub use compiler::{compile, CompileOptions, CompileOutput, Generate, Renderer, Wrapper};
pub use error::{CompileError, CompileErrorKind};
pub use semantic_trace::{
    CallbackDecision, ComponentRenderSite, ExecutionSite, ExecutionSiteKind, OwnerEstablishment,
    SemanticTrace, SourceSpan, TerminalDecision, ValueDecision, SEMANTIC_TRACE_VERSION,
};

#[cfg(feature = "node")]
pub use node_adapter::*;
