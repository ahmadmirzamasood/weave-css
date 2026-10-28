//! Compiler library for the Weave stylesheet language.
//!
//! Status: values, scopes and interpolation (phase 04). [`compile`]
//! tokenizes, parses, evaluates and emits the plain-CSS subset (`rule`
//! blocks, declarations, known and unknown at-rules, `opaque` blocks,
//! comments) together with `bind` statements, expressions, `#{…}`
//! interpolation in every CSS context and the `css.*` sequence
//! constructors. Other Weave constructs are reported as E5001 (not
//! implemented by this build); they are never silently ignored.

#![forbid(unsafe_code)]

mod builtins;
mod compile;
mod css_text;
pub mod diagnostic;
mod emit;
mod eval;
mod expr;
mod ir;
mod lexer;
mod number;
pub mod options;
mod parser;
pub mod resolve;
pub mod source;
mod syntax;
mod value;

pub use compile::{Compilation, CompileOutput, compile, resolve_diagnostic};
pub use diagnostic::{Code, Diagnostic, Diagnostics, Label, Severity};
pub use options::{CompileOptions, Limit, LimitError, OutputStyle, ResourceLimits};
pub use resolve::{LoadedModule, MemoryResolver, ModuleId, ModuleResolver, ResolveError};
pub use source::{FileId, LineCol, SourceFile, SourceManager, Span};

/// The version of this crate.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// The language specification version this compiler targets.
pub const LANGUAGE_VERSION: &str = "0.1";
