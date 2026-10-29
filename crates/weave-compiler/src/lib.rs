#![doc = include_str!("../README.md")]
#![forbid(unsafe_code)]

mod builtins;
mod compile;
mod css_text;
pub mod diagnostic;
mod emit;
mod eval;
mod expr;
#[cfg(feature = "fuzzing")]
#[doc(hidden)]
pub mod fuzzing;
mod ir;
mod lexer;
mod modules;
mod number;
pub mod options;
mod parser;
pub mod resolve;
mod selector;
pub mod source;
pub mod source_map;
mod syntax;
mod value;

pub use compile::{
    Compilation, CompileOutput, Phase, compile, compile_observed, compile_with_cache,
    resolve_diagnostic,
};
pub use diagnostic::{Code, Diagnostic, Diagnostics, Label, Severity};
pub use modules::{CacheStats, DependencyGraph, ModuleCache};
pub use options::{CompileOptions, Limit, LimitError, OutputStyle, ResourceLimits};
pub use resolve::{LoadedModule, MemoryResolver, ModuleId, ModuleResolver, ResolveError};
pub use source::{FileId, LineCol, SourceFile, SourceManager, Span};
pub use source_map::{JsonOptions, Mapping, OriginalPosition, Position, SourceMap};

/// The version of this crate.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// The language specification version this compiler targets.
pub const LANGUAGE_VERSION: &str = "0.1";
