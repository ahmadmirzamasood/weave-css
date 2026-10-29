//! Compiles the Rust examples in `docs/LIBRARY.md` as documentation tests.
//!
//! This crate exists only so `cargo test --workspace` runs them; it is not
//! published and has no API.

#![forbid(unsafe_code)]
#![doc = include_str!("../../../docs/LIBRARY.md")]
