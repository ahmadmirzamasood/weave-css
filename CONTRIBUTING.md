# Contributing

Development happens in public GitHub branches and pull requests. Discuss language changes in an issue before changing semantics, and update the specification documents and fixtures together. Submit focused changes with fixtures, documentation and exact validation results. Use `feat:`, `fix:`, `docs:`, `test:` or `perf:` commit prefixes when useful.

Run `python3 scripts/check_spec.py` to check fixtures and documentation links (CI runs it on every pull request).

## Rust workspace

- `crates/weave-compiler`: the compiler library (package `weave-compiler`). `crates/weave-cli`: the `weave` binary (package `weave-css`). `crates/weave-bench`: benchmark tooling and `crates/weave-docs`: the harness that compiles the library guide's examples; neither is published. `fuzz/` is a separate workspace.
- Toolchain: `rust-toolchain.toml` pins Rust 1.99.1 with rustfmt and Clippy; rustup installs it automatically (or run `rustup toolchain install`). The minimum supported Rust version is 1.85 (`rust-version` in Cargo.toml), checked in CI.
- Checks, all required before merging:

  ```bash
  cargo fmt --all --check
  cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
  cargo test --workspace --all-features --locked
  python3 scripts/check_deps.py
  python3 scripts/check_spec.py
  python3 scripts/check_release.py
  ```

- Supported platforms: Linux, macOS and Windows on GitHub-hosted runners (`ubuntu-latest`, `macos-latest`, `windows-latest`), each running fmt, Clippy and tests; MSRV tests run on Linux. The crates define no Cargo features, so `--all-features` equals the default build and no feature matrix is needed; add one when features are introduced.
- Project code is safe Rust (`unsafe_code` is forbidden) and must not panic, `unwrap`/`expect` or index unchecked on user-controlled input; Clippy enforces this outside tests.
- Dependencies: the workspace currently has none. A new dependency needs a review of its license, build script and native requirements, an entry in `scripts/check_deps.py` and a changelog note. The compiler must never require a JavaScript, C or C++ runtime or toolchain. Include performance evidence for optimization claims. Do not submit generated/private assets or fixtures without appropriate rights and provenance. Contributions are made under the project's MIT license; retain third-party notices. Be respectful and explain technical disagreements with examples.

Documentation examples are tests: every `weave` block followed by a `css` (or `text`) block in README.md and docs/TUTORIAL.md is compiled by `crates/weave-compiler/tests/docs.rs`, and the Rust blocks in docs/LIBRARY.md are doctests (run by `cargo test --workspace`). If you change behavior, change the documentation in the same pull request and the checks tell you what is stale. Changes to the language need an issue first, an update to docs/LANGUAGE.md and its fixtures, and a changelog entry under "Unreleased"; read docs/COMPATIBILITY-POLICY.md for what counts as a breaking change. The release procedure is docs/RELEASING.md.

Maintainers review compatibility and security implications before merging. Never force-push main. Report security issues privately as described in SECURITY.md.
