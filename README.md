# Weave — Rust CSS language development kit

Status: planning package, not an implemented compiler. Weave and `.weave` are working names; naming clearance and package availability are not established. Proposed repository: `weave-css`.

Build an independently designed, fast CSS preprocessor entirely in Rust. Emit standard CSS without a JavaScript, C, or C++ runtime. Existing Rust crates are allowed after review; do not wrap an existing preprocessor or reproduce its syntax/compatibility contract.

## Start with Claude Code

1. Extract this directory and open Claude Code from its root.
2. Read [START-HERE.md](START-HERE.md).
3. Execute [phase 00](prompts/phase-00-github.md) to publish this package to a new public GitHub repository.
4. Execute phases 01–11 in order. Each prompt is a complete task, with prerequisites and acceptance gates. Use a fresh session per phase if helpful.
5. Keep implementation, fixtures, examples, decisions, benchmark reports, and these documents in that repository.

## Documents

- [Agent instructions](instructions/AGENT.md), loaded by [CLAUDE.md](CLAUDE.md)
- [Project memory](MEMORY.md): actual status and handoff, not an automatic Claude memory file
- [Product requirements](PRD.md)
- [Development plan](DEV-PLAN.md)
- [Language design](docs/LANGUAGE.md) and [architecture](docs/ARCHITECTURE.md)
- [Testing](docs/TESTING.md) and [performance](docs/PERFORMANCE.md)
- [Security](SECURITY.md), [release](RELEASE.md), [contribution guide](CONTRIBUTING.md)
- [MIT license](LICENSE), [changelog](CHANGELOG.md), [sources](docs/SOURCES.md)

No repository has been created by this package. No build, benchmark, or compiler correctness result is claimed. All compiler commands below become available only when their corresponding phase is implemented.
