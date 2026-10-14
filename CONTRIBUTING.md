# Contributing

Development happens in public GitHub branches and pull requests. Discuss language changes in an issue before changing semantics, and update the specification documents and fixtures together. Submit focused changes with fixtures, documentation and exact validation results. Use `feat:`, `fix:`, `docs:`, `test:` or `perf:` commit prefixes when useful.

Once the Rust workspace exists, install the pinned Rust toolchain and run `cargo fmt --all --check`, `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` and `cargo test --workspace --all-features --locked`. Include performance evidence for optimization claims. Do not submit generated/private assets or fixtures without appropriate rights and provenance. Contributions are made under the project's MIT license; retain third-party notices. Be respectful and explain technical disagreements with examples.

Maintainers review compatibility and security implications before merging. Never force-push main. Report security issues privately as described in SECURITY.md.
