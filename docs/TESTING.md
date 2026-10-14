# Testing strategy

Maintain fixtures with `.weave`, expected `.css`, and structured diagnostic expectations. The fixture layout and `expected.diag` format are defined in [tests/fixtures/spec/README.md](../tests/fixtures/spec/README.md); phase 01 added 35 specification fixtures that await a runner. Test output semantics, not only prettified strings. Each phase adds negative and boundary cases as well as successful examples. Every supported construct needs coverage; unsupported constructs must produce explicit diagnostics or use the documented opaque path.

Required corpus: Unicode/escapes/CRLF; strings containing braces and semicolons; comments; data URLs; custom properties; var/calc/env; duplicate fallbacks; !important; selector lists and specificity; interleaved nesting; all known at-rule contexts; map/list ordering; scope; unit arithmetic; module diamonds/cycles/symlinks; output limits; source maps for Unicode and expansion; failed watch builds retaining last good output.

Use a standards CSS parser to validate known output syntax. It may lag future CSS: opaque-preservation tests must not be rejected solely by that parser. Add browser-computed-style checks for specificity/order where an integration harness is justified. Never use Sass/Less output as this language's semantic oracle.

Fuzz lexer/parser and compile entry points with bounded options. Require termination, no panics, stable diagnostics, and deterministic successful output. Seed dictionaries with CSS and Weave tokens. Persist minimized crashes as regression fixtures. Record fuzz duration, seed/corpus, toolchain and outcomes; a short smoke run is not exhaustive assurance.

CI: fmt, clippy, tests on supported platforms; minimum supported Rust check; deterministic output; dependency/security/license checks; documentation/example compilation. Keep performance reporting off noisy shared-runner blocking gates until variance is characterized. Pin action SHAs and use read-only permissions for PR jobs.
