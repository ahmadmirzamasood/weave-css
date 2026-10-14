# Phase 09: Measure and optimize compiler performance

Copy this file into Claude Code, or ask it to read and execute this file.

## Prompt

You are implementing Weave, an independent Rust CSS preprocessor. Read CLAUDE.md, instructions/AGENT.md, MEMORY.md, PRD.md, DEV-PLAN.md, SECURITY.md and the relevant design documents before editing. Inspect the current repository; do not assume memory proves code works.

### Prerequisites

Phase 08 gates pass and representative workloads exist.

### Work to perform

Implement reproducible benchmark generators and raw sample reporting per docs/PERFORMANCE.md. Measure cold/warm/incremental time, memory and output sizes. Profile before optimizing; implement correct dependency-driven invalidation and cache keys including options/compiler version/source identity. Test leaf and shared-module edits, deletes, renames, negative lookup recovery, changed limits, and source-map relocation. Add parallel independent work only if it improves measured workloads while retaining deterministic output.

### Acceptance gates

Before/after report includes hardware/toolchain/revision/corpus/sample count/variance and correctness checks; warm results are not mislabeled cold; cache tests prove no stale output; explicit budgets established on a reference machine; small-file regressions investigated.

### Scope boundaries

No invented benchmark numbers, unsafe code, or unsupported speed comparisons. Record trade-offs and unresolved targets honestly.

### Handoff

Complete this phase only. Update MEMORY.md with actual state, decisions, exact validation commands/results, unresolved issues and next phase. Update relevant specs, changelog and the DEV-PLAN checklist. Commit and push the phase branch to the configured public repository; preserve unrelated work and remote history. If external publication is blocked, record it separately from implementation status. Report changed behavior, evidence, commit/branch and the next prompt. Never claim a gate passed unless it was run or explicitly verified as a document review.
