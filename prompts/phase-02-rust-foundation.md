# Phase 02: Create the Rust workspace and CI

Copy this file into Claude Code, or ask it to read and execute this file.

## Prompt

You are implementing Weave, an independent Rust CSS preprocessor. Read CLAUDE.md, instructions/AGENT.md, MEMORY.md, PRD.md, DEV-PLAN.md, SECURITY.md and the relevant design documents before editing. Inspect the current repository; do not assume memory proves code works.

### Prerequisites

Phase 01 specification accepted and fixtures available.

### Work to perform

Create compiler and CLI crates with a minimal truthful version/help command; no fake compilation success. Verify current Rust stable, choose and record an MSRV and exact development toolchain. Add source IDs/spans, diagnostics, options/resource limits, resolver interface, output types and typed unsupported-feature errors. Forbid unsafe in project crates. Add Cargo.lock and reproducible CI with verified full action SHAs and minimal permissions. Define supported Linux/macOS/Windows matrix and feature combinations. Audit any added dependency for license and native build requirements.

### Acceptance gates

Cargo format/clippy/tests pass; CLI help/version work; Unicode span/diagnostic tests pass; CI syntax and resolved action SHAs verified; unimplemented compilation reports an explicit error; no placeholder passing test for compiler correctness.

### Scope boundaries

Use a small dependency set and no premature evaluator/VM. Record exact commands and toolchain.

### Handoff

Complete this phase only. Update MEMORY.md with actual state, decisions, exact validation commands/results, unresolved issues and next phase. Update relevant specs, changelog and the DEV-PLAN checklist. Commit and push the phase branch to the configured public repository; preserve unrelated work and remote history. If external publication is blocked, record it separately from implementation status. Report changed behavior, evidence, commit/branch and the next prompt. Never claim a gate passed unless it was run or explicitly verified as a document review.
