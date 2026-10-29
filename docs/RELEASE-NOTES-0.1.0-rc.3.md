# Weave 0.1.0-rc.3 release notes

**Release candidate. Not published:** no package is on a registry and no GitHub release exists. These notes describe what the candidate contains and what is unproven, so that the decision to publish can be made on facts.

## Candidate commit

The candidate is one exact commit of the `phase/11-release` branch (later `main`). A commit cannot contain its own hash, so it is pinned outside the tree: the release workflow takes it as `expected_commit` and builds nothing else, the draft release targets it, and the maintainer's phase 11 handoff records it. To check a checkout is the candidate: `git rev-parse HEAD` must equal that recorded value and `git status` must be clean. [RELEASING.md](RELEASING.md) has the procedure.

## What it is

Weave is a stylesheet language that compiles `.weave` files to standard CSS, with a Rust compiler library (`weave-compiler`) and the `weavec` command (package `weave-css`). Pure Rust, no third-party dependencies, `unsafe` forbidden, MIT licensed.

## Supported features

All of the [v0.1 language](LANGUAGE.md):

- `bind` immutable values: numbers with units, strings, hex colors, booleans, lists, ordered maps, CSS fragments; expressions with documented precedence; `#{…}` interpolation escaped per context.
- Typed `fn` functions (defaults, named arguments, recursion), `when`/`otherwise`, `each`, `range`, `fail`/`warn`, and the [built-in library](BUILTINS.md) (`css`, `math`, `list`, `map`, `string`, `color`, `meta`). The `color` built-ins (`rgb`, `mix`, `red`, `green`, `blue`, `alpha`) were added in this candidate: exact integer arithmetic, ties rounded toward +infinity, checked by hand-derived fixtures and independent oracles.
- `recipe`/`apply`/`slot` reusable rules with content blocks.
- Nested rules lowered to flat CSS with CSS Nesting specificity (checked against Chrome in CI), nested `@media`/`@supports`/`@container`/`@layer`/`@starting-style`.
- All CSS preserved: declarations, custom properties, `!important`, known at-rules by their body grammar, unknown at-rules and `opaque { }` blocks verbatim.
- Modules: `load`, `export`, namespaces, cycle detection, each module built once, root-confined paths.
- Tools: `weavec build|check|watch`, stdin/stdout, atomic output, human or JSON diagnostics with stable codes, ECMA-426 source maps, dependency lists, `--limit` overrides. Library: `compile`, caching, dependency graph, resolvers, source maps ([LIBRARY.md](LIBRARY.md)).
- Syntax highlighting for VS Code-compatible editors (TextMate grammar; not a language server).

## Platforms

Tested in CI, and the only ones with prebuilt binaries planned: Linux x86_64 (glibc), macOS Apple silicon, Windows x86_64 (MSVC). MSRV 1.85 is tested on Linux. See [INSTALL.md](INSTALL.md#supported-platforms).

## Known limitations

1. **Large inputs need a raised step limit.** Plain CSS above roughly 333,000 simple rules (about 3.6 MB) exceeds the default 1,000,000 evaluation steps and fails with `E8004`; raise it with `--limit evaluation-steps=N` ([CLI.md](CLI.md#resource-limits)). The default is unchanged on purpose.
2. **Root confinement is path-based.** A process that can modify the module tree during a build can defeat it (time-of-check/time-of-use). Weave is not a sandbox for trees others can write concurrently.
3. `@scope` nested inside rules is rejected (E6006); `@font-feature-values` internals are preserved verbatim, not parsed. Equal-specificity selector expansion is not optimized (parent lists with several selectors use `:is()`).
4. `watch` polls file metadata: a same-length edit within the file system's timestamp granularity can be noticed only at the next change; edits made during a build to files not yet watched are missed.
5. The language grammar file is an approximate EBNF, and fixture expectations had no independent human review beyond the author and automated cross-checks (browser, independent CSS parser).
6. The library API and grammar are not stable before 1.0 ([COMPATIBILITY-POLICY.md](COMPATIBILITY-POLICY.md)).
7. No prebuilt binaries for Intel macOS, Linux musl/ARM or Windows ARM; untested, so unclaimed.
8. `cargo package` prints "ignoring test … not included in the published package" warnings: the packaged crates intentionally ship no tests, which need the repository's fixtures.

## Decisions needed before publication

- **Binary name (decided for this candidate):** the command is `weavec`. The earlier choice `weave` collides with the `weave-cli` crate (an unrelated tool that also installs `weave`), a Homebrew formula named `weave` and other projects (checked 2026-10-29; [INSTALL.md](INSTALL.md#the-binary-name)). The language, `.weave` extension and package names are unchanged. Whether to rename the language itself before 1.0 remains open.
- **Registry names must be re-checked immediately before publishing.** On 2026-10-29 `weave-css` and `weave-compiler` were free on crates.io (and `weave-css` on npm and PyPI); `weave` and `weave-cli` were taken.
- Repository setup for publication: a `release` environment with a required reviewer, and scoped crates.io credentials held by a maintainer.

## Benchmark evidence

On one Windows laptop, interleaved against the previous `main`, the candidate is within noise on all workloads except one cold slowdown of 8.8 % (selector-expansion), under the 10 % investigation threshold; memory unchanged; output byte-identical. Earlier optimization work cut cold build times by 9–43 % on larger workloads. No other tool was measured and no other machine was used. Full method, tables and caveats: [BENCHMARKS.md](BENCHMARKS.md).

## Security evidence

From the phase 10 review (local report) and the CI/test suite:

- Resource limits on input, files, modules, nesting, calls, evaluation steps, selector expansion, output, collections, strings and diagnostics, each with a test; no ambient network, shell, environment or clock access in the compiler.
- Filesystem root confinement including symbolic links and Windows junctions; atomic outputs and collision prevention; terminal-control escaping in diagnostics.
- Fuzzing (libFuzzer with ASan, lexer, parser and compiler targets): about 6.6 million executions in two campaigns, no crash, hang or out-of-memory. Fuzzing is bounded evidence, not proof.
- Findings fixed: quadratic map-literal and argument checks and graph processing (a 1 MiB input could take minutes), special files blocking a build, unescaped path in a watch status line, unescaped U+2028/2029. No critical or high findings. Accepted and documented: the path-based race above, and the default step limit above.
- `cargo audit` on both lockfiles reported no advisories; a secret scan of the history and tree found none; dependencies are none (dev-only test crates are listed in [THIRD-PARTY-NOTICES.md](../THIRD-PARTY-NOTICES.md)); CI actions are pinned to full commit SHAs with read-only default permissions.
- Not done: independent third-party audit; fuzzing of the CLI argument parser beyond unit tests; any claim of safety against concurrent writers or multi-tenant use.

## Security reporting

Report vulnerabilities privately through GitHub: <https://github.com/ahmadmirzamasood/weave-css/security/advisories/new>. Do not use public issues for exploitable details. Scope, what to include and what to expect: [SECURITY.md](../SECURITY.md). No response-time guarantee is made.

## Migration implications

- This is the first release, so there is nothing to migrate from. Weave is not compatible with Sass, Less or other preprocessors; there is no converter. Plain CSS becomes Weave by prefixing selectors of style rules with `rule` ([COMPATIBILITY-POLICY.md](COMPATIBILITY-POLICY.md#migration)).
- Anything may change between release candidates and before 1.0 as described in the compatibility policy: the library API in particular, and the names (`weavec`, `.weave`, package names) are working choices.
- `color.*` calls that previously failed with E5001 now compile; no program that compiled before changes meaning.
- If you adopted the repository before rc.2: the CLI package is now `weave-css` (`cargo run -p weave-css`; was `-p weave-cli`) and the command is `weavec` (was `weave`), including the `[weavec]` prefix of `watch` status lines. Flags and CSS output are unchanged. 0.1.0-rc.1 and 0.1.0-rc.2 were prepared but never published.

## How this candidate was verified

- `cargo fmt --check`, `cargo clippy -D warnings`, `cargo test --workspace --all-features --locked` (all tests plus doctests passing on the Windows development machine; Linux, macOS and Windows in CI), MSRV 1.85 on Linux CI.
- Specification and fixture consistency (100 fixtures), dependency policy, editor files, release consistency (`scripts/check_release.py`).
- Package dry run for both published crates and inspection of their contents; clean-environment build and install from the packaged crates and the source archive; the release workflow rehearsal on the pull request (builds, version and example smoke tests of each binary, Linux reproducibility). Results are recorded in the pull request and the phase 11 handoff, not in this file.
- Examples in the README, tutorial and library guide are compiled by tests.

## Install

Build from source today: [INSTALL.md](INSTALL.md). Registry packages and binaries exist only after the maintainers publish ([RELEASING.md](RELEASING.md)).

## Rollback

Before publication the draft is simply deleted. After publication tags and assets are never moved; a bad version is yanked from crates.io, superseded by a fixed release and described in an advisory ([RELEASING.md](RELEASING.md#rollback-and-incident-handling)).
