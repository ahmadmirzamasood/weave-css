# Phase 05: Implement functions, control flow and reusable recipes

Copy this file into Claude Code, or ask it to read and execute this file.

## Prompt

You are implementing Weave, an independent Rust CSS preprocessor. Read CLAUDE.md, instructions/AGENT.md, MEMORY.md, PRD.md, DEV-PLAN.md, SECURITY.md and the relevant design documents before editing. Inspect the current repository; do not assume memory proves code works.

### Prerequisites

Phase 04 gates pass.

### Work to perform

Implement pure typed functions, defaults/named arguments, returns, lexical capture, recursion limits, when/otherwise, bounded iteration/ranges and recipes with content slots. Use caller lexical scope for passed content and preserve definition/call-site provenance. Add selected deterministic built-ins for map/list/string/number operations, documented under docs/BUILTINS.md. Enforce step/output limits during expansion, before unbounded allocations.

### Acceptance gates

Positive and negative argument/type/control-flow/slot fixtures pass; short circuit avoids invalid branches; recursion and expansion bombs fail gracefully; functions cannot emit CSS or access ambient I/O; recipe expansion order is deterministic.

### Scope boundaries

Do not add arbitrary plugins, mutable globals or a copied external standard library.

### Handoff

Complete this phase only. Update MEMORY.md with actual state, decisions, exact validation commands/results, unresolved issues and next phase. Update relevant specs, changelog and the DEV-PLAN checklist. Commit and push the phase branch to the configured public repository; preserve unrelated work and remote history. If external publication is blocked, record it separately from implementation status. Report changed behavior, evidence, commit/branch and the next prompt. Never claim a gate passed unless it was run or explicitly verified as a document review.
