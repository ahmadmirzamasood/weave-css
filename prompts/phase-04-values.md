# Phase 04: Implement values, scopes and interpolation

Copy this file into Claude Code, or ask it to read and execute this file.

## Prompt

You are implementing Weave, an independent Rust CSS preprocessor. Read CLAUDE.md, instructions/AGENT.md, MEMORY.md, PRD.md, DEV-PLAN.md, SECURITY.md and the relevant design documents before editing. Inspect the current repository; do not assume memory proves code works.

### Prerequisites

Phase 03 gates pass; type/numeric contracts fixed.

### Work to perform

Implement immutable lexical bindings, ordered maps, indexed lists, numeric dimensions, strings/colors/booleans, expression precedence and short-circuiting. Add explicit CSS sequence constructors and context-aware interpolation. Separate compiler expressions from CSS runtime functions. Implement unit/overflow/divide-by-zero/type diagnostics and collection bounds. Resolve identifiers to stable symbols where practical without overengineering. Wire fixture runner to value-focused specification examples.

### Acceptance gates

Scope/shadowing/type/units/indexing tests pass; deterministic map iteration; literal var/calc/env survive; bare CSS identifiers never resolve as variables; dangerous interpolation cannot inject declaration boundaries; Unicode and numeric edge cases covered.

### Scope boundaries

No functions, loops or modules yet; existing unsupported-feature diagnostics remain accurate.

### Handoff

Complete this phase only. Update MEMORY.md with actual state, decisions, exact validation commands/results, unresolved issues and next phase. Update relevant specs, changelog and the DEV-PLAN checklist. Commit and push the phase branch to the configured public repository; preserve unrelated work and remote history. If external publication is blocked, record it separately from implementation status. Report changed behavior, evidence, commit/branch and the next prompt. Never claim a gate passed unless it was run or explicitly verified as a document review.
