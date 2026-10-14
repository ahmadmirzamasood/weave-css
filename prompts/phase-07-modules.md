# Phase 07: Implement module resolution and dependency graph

Copy this file into Claude Code, or ask it to read and execute this file.

## Prompt

You are implementing Weave, an independent Rust CSS preprocessor. Read CLAUDE.md, instructions/AGENT.md, MEMORY.md, PRD.md, DEV-PLAN.md, SECURITY.md and the relevant design documents before editing. Inspect the current repository; do not assume memory proves code works.

### Prerequisites

Phase 06 gates pass.

### Work to perform

Implement load/export/namespaces, immutable imports, canonical module identity, relative explicit-extension resolution, cycle chains and deterministic load-once CSS emission. Enforce configured filesystem root including symlink escapes; implement in-memory resolver tests. Track dependency graph and failures. Validate native @import/@charset/@namespace/layer ordering without silently reordering CSS. Cache parsed modules with explicit invalidation hooks; do not claim persistent incremental correctness before phase 09.

### Acceptance gates

Diamond imports emit dependency CSS once; order is stable; exported/private symbols behave correctly; cycles/missing files report importer spans; traversal/symlink tests pass; changing a dependency cannot reuse stale parsed contents; virtual resolver works without filesystem access.

### Scope boundaries

No network loading, implicit package discovery or module registry.

### Handoff

Complete this phase only. Update MEMORY.md with actual state, decisions, exact validation commands/results, unresolved issues and next phase. Update relevant specs, changelog and the DEV-PLAN checklist. Commit and push the phase branch to the configured public repository; preserve unrelated work and remote history. If external publication is blocked, record it separately from implementation status. Report changed behavior, evidence, commit/branch and the next prompt. Never claim a gate passed unless it was run or explicitly verified as a document review.
