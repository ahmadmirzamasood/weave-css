# Changelog

## Unreleased

- Publish the repository with license, security policy and contribution guide.
- Add the Weave v0.1 language specification: docs/LANGUAGE.md, docs/grammar.ebnf, docs/TYPES.md, docs/CSS-COMPATIBILITY.md; 35 specification fixtures in tests/fixtures/spec (pending a compiler runner).
- Notable v0.1 choices: `rule` takes a raw CSS selector prelude; statement keywords always start statements; `load` must precede other statements; CSS fragment literal `` `…` ``; `\#{` literal marker; parent lists with more than one selector lower to `:is()`.
- Add CI: specification consistency checks (`scripts/check_spec.py`) and Rust format/clippy/test jobs on Linux, macOS and Windows that activate once a Cargo workspace exists.
- Add the Rust workspace (phase 02): `weave-compiler` library and `weave-cli` with a `weave` binary. Development toolchain Rust 1.99.1 (rust-toolchain.toml), MSRV 1.85, edition 2024, no third-party dependencies, unsafe code forbidden.
- Compiler foundation: source manager with byte spans, line/column (scalar values) and UTF-16 columns; E0001 invalid UTF-8 and E0006 U+0000 detection; diagnostics with stable codes, labels, notes and terminal-safe rendering; validated resource limits with the LANGUAGE.md §17 defaults; module resolver trait with load-path validation, an in-memory resolver and a root-confined filesystem resolver in the CLI.
- New diagnostic E5001 (feature not implemented by this build), LANGUAGE.md §16. `weave build`/`weave check` validate the input and then fail with E5001; no CSS is produced yet.
- CI: pinned toolchain from rust-toolchain.toml, MSRV 1.85 test job and dependency policy check (`scripts/check_deps.py`).
- The Weave language itself is not implemented yet.
