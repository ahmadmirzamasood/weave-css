//! Compiler library for the Weave stylesheet language.
//!
//! Status: plain-CSS subset (phase 03). [`compile`] tokenizes, parses and
//! emits `rule` blocks, declarations, known and unknown at-rules, `opaque`
//! blocks and comments. Other Weave constructs are reported as E5001 (not
//! implemented by this build); they are never silently ignored.

#![forbid(unsafe_code)]

mod compile;
mod css_text;
pub mod diagnostic;
mod emit;
mod lexer;
pub mod options;
mod parser;
pub mod resolve;
pub mod source;
mod syntax;

pub use compile::{Compilation, CompileOutput, compile, resolve_diagnostic};
pub use diagnostic::{Code, Diagnostic, Diagnostics, Label, Severity};
pub use options::{CompileOptions, Limit, LimitError, OutputStyle, ResourceLimits};
pub use resolve::{LoadedModule, MemoryResolver, ModuleId, ModuleResolver, ResolveError};
pub use source::{FileId, LineCol, SourceFile, SourceManager, Span};

/// The version of this crate.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// The language specification version this compiler targets.
pub const LANGUAGE_VERSION: &str = "0.1";
