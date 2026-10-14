# ADR 0001: Independent Rust language

Status: accepted project direction; detailed grammar remains proposed.

The user requires a new language with its own extension, full CSS expressiveness, reusable compile-time programming, speed, and public open-source development. Implement the compiler and CLI in Rust. Do not provide Sass/Less compatibility. Use Weave and `.weave` as provisional names and MIT for original project contributions. Use explicit interpolation to separate CSS tokens from compile-time expressions. Prefer measurable performance and deterministic semantics over speculative complexity.

Consequences: existing preprocessor stylesheets are not direct inputs; documentation and migration examples must make this clear. Name/package availability and grammar details must be checked before launch.
