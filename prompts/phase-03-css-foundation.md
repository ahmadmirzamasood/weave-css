# Phase 03: Implement tokenizer, parser and plain CSS emission

Copy this file into Claude Code, or ask it to read and execute this file.

## Prompt

You are implementing Weave, an independent Rust CSS preprocessor. Read CLAUDE.md, instructions/AGENT.md, MEMORY.md, PRD.md, DEV-PLAN.md, SECURITY.md and the relevant design documents before editing. Inspect the current repository; do not assume memory proves code works.

### Prerequisites

Phase 02 gates pass.

### Work to perform

Implement UTF-8 scanning, token spans, syntax parsing, rule/declaration structures, known at-rule body classification and opaque balanced CSS blocks. Emit plain CSS from the subset without expressions. Preserve custom-property bytes, duplicate declarations, escapes, URLs, unknown at-rules and comments where required. Implement bounded parser recovery and source locations. Add a fixture runner and use independent CSS validation for known syntax. Wire minimal CLI build/check for this supported subset; reject unsupported Weave constructs explicitly.

### Acceptance gates

Plain stylesheet fixtures compile correctly; malformed inputs terminate with spans; braces/semicolons in strings/comments/URLs survive; unknown syntax preservation passes; no accidental evaluation of CSS values; resource-depth and input-size limits tested.

### Scope boundaries

Do not flatten nesting or evaluate variables yet. Never silently ignore a compile-time construct.

### Handoff

Complete this phase only. Update MEMORY.md with actual state, decisions, exact validation commands/results, unresolved issues and next phase. Update relevant specs, changelog and the DEV-PLAN checklist. Commit and push the phase branch to the configured public repository; preserve unrelated work and remote history. If external publication is blocked, record it separately from implementation status. Report changed behavior, evidence, commit/branch and the next prompt. Never claim a gate passed unless it was run or explicitly verified as a document review.
