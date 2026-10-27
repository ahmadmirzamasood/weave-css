//! Compiler library for the Weave stylesheet language.
//!
//! Status: foundation only. This crate defines source files, spans,
//! diagnostics, options, resource limits and the module resolver interface.
//! [`compile`] validates the entry module's source text and then reports
//! E5001 (not implemented); it never produces CSS yet.

#![forbid(unsafe_code)]

mod compile;
pub mod diagnostic;
pub mod options;
pub mod resolve;
pub mod source;

pub use compile::{Compilation, CompileOutput, compile, resolve_diagnostic};
pub use diagnostic::{Code, Diagnostic, Diagnostics, Label, Severity};
pub use options::{CompileOptions, Limit, LimitError, OutputStyle, ResourceLimits};
pub use resolve::{LoadedModule, MemoryResolver, ModuleId, ModuleResolver, ResolveError};
pub use source::{FileId, LineCol, SourceFile, SourceManager, Span};

/// The version of this crate.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// The language specification version this compiler targets.
pub const LANGUAGE_VERSION: &str = "0.1";
