# Phase 06: Implement nesting and full CSS rule contexts

Copy this file into Claude Code, or ask it to read and execute this file.

## Prompt

You are implementing Weave, an independent Rust CSS preprocessor. Read CLAUDE.md, instructions/AGENT.md, MEMORY.md, PRD.md, DEV-PLAN.md, SECURITY.md and the relevant design documents before editing. Inspect the current repository; do not assume memory proves code works.

### Prerequisites

Phase 05 gates pass; nesting specification revision recorded.

### Work to perform

Implement structural selectors and nested rule lowering with parent references and selector-list specificity preservation. Preserve declarations before/between/after nested rules. Implement media/supports/container/layer/scope nesting interactions and keyframes/font-face/property/page contexts per compatibility matrix. Maintain opaque forwarding for future constructs. Add computed-style integration cases where needed to verify cascade and specificity, not only output snapshots.

### Acceptance gates

Mixed-specificity parent lists, pseudo arguments, ampersands and interleaved declarations have semantic evidence; known at-rule fixtures pass; opaque future syntax remains unchanged; selector growth is bounded; all supported Weave rules lower to ordinary CSS with no compile-time constructs left.

### Scope boundaries

No naive textual selector replacement or speculative hoisting/reordering. Browser compatibility follows emitted CSS capabilities.

### Handoff

Complete this phase only. Update MEMORY.md with actual state, decisions, exact validation commands/results, unresolved issues and next phase. Update relevant specs, changelog and the DEV-PLAN checklist. Commit and push the phase branch to the configured public repository; preserve unrelated work and remote history. If external publication is blocked, record it separately from implementation status. Report changed behavior, evidence, commit/branch and the next prompt. Never claim a gate passed unless it was run or explicitly verified as a document review.
