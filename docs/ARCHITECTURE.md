# Architecture

## Workspace

Start with `crates/weave-compiler` (library) and `crates/weave-cli` (binary named weave). Names are local working names, not claimed crates.io registrations. Add `tests/fixtures`, `examples`, `benches`, and later `fuzz` when used. Commit Cargo.lock for reproducible CLI/workspace builds.

## Pipeline

Source manager -> lexer -> syntax tree -> name/type analysis -> module graph/evaluation -> CSS IR -> emitter/source map.

SourceManager owns immutable UTF-8 buffers. Spans carry file ID and byte start/end. Maintain line tables for diagnostics; source-map positions need explicit UTF-16 column conversion where required. Intern identifiers if measurements support it; store arena IDs instead of self-referential Rust structures.

Parser uses recursive descent for statements, Pratt parsing for expressions, and a CSS-aware component-value scanner for literal regions. Balanced blocks must respect strings, escapes, comments and URL tokens. Preserve trivia where semantically relevant. A valid tree is not permission to discard unknown CSS.

Evaluator owns typed values, lexical frames, a step counter and call stack. Output nodes retain definition and call-site provenance. Module resolver is an injected trait; the core does not read files. CLI filesystem resolver enforces the selected root. Cache keys include source identity/content, compiler version, options and dependency changes; span/source-map changes also invalidate relevant outputs.

CSS IR distinguishes qualified rules, declaration segments, known at-rules and opaque nodes. Selector operations are structural. Emission preserves cascade ordering and unknown syntax. Source maps track expansion chains with a documented primary-origin policy.

## Proposed API shape

`compile(source, options, resolver) -> Result<CompileOutput, Vec<Diagnostic>>`

Define concrete source identity and owned/borrowed types during phase 02. CompileOutput contains CSS, optional source map, dependency identities and warnings. Options include resource limits and output mode. Diagnostics contain stable code, severity, primary span, secondary labels and notes. Check mode emits no files.

## Dependency policy

Prefer small maintained Rust libraries with compatible licenses. Review transitive native build requirements and build scripts. CSS parser libraries may support validation/token handling but cannot substitute an existing preprocessor engine. Runtime code is Rust; shell/YAML/editor configuration may support development. A test-only browser harness is allowed for semantic verification. Keep optional integrations out of the core dependency path.
