# Phase 01: Freeze the language contract

Copy this file into Claude Code, or ask it to read and execute this file.

## Prompt

You are implementing Weave, an independent Rust CSS preprocessor. Read CLAUDE.md, instructions/AGENT.md, MEMORY.md, PRD.md, DEV-PLAN.md, SECURITY.md and the relevant design documents before editing. Inspect the current repository; do not assume memory proves code works.

### Prerequisites

Phase 00 publication complete, or explicitly recorded external blocker; all design documents present.

### Work to perform

Turn docs/LANGUAGE.md into a normative v0.1 specification. Resolve every listed ambiguity with concrete examples: grammar, declaration versus expression scanning, literal interpolation markers, escaping in each context, numeric representation/rounding, types/coercions, lexical scope, recursion, namespaced calls, map access, CSS sequences, recipes/content slots, at-rule contexts, nesting specificity, module emission and load ordering.
Create docs/grammar.ebnf, docs/TYPES.md and docs/CSS-COMPATIBILITY.md. Verify current CSS Nesting and source-map primary specifications and record exact revisions. Add at least 20 paired language fixtures, including errors and a multi-module example; mark them as specification fixtures pending a compiler runner. Update ADRs and PRD traceability. Independently design semantics rather than use Sass/Less as an oracle.

### Acceptance gates

No unresolved syntax ambiguity needed by phase 02/03; example outputs manually reviewed; at least 20 well-described fixtures; property/declaration/at-rule context matrix; explicit interpolation-injection policy; spec files and memory agree.

### Scope boundaries

Do not implement the compiler yet or claim fixtures passed executable tests. Decide ordinary design questions and document rationale.

### Handoff

Complete this phase only. Update MEMORY.md with actual state, decisions, exact validation commands/results, unresolved issues and next phase. Update relevant specs, changelog and the DEV-PLAN checklist. Commit and push the phase branch to the configured public repository; preserve unrelated work and remote history. If external publication is blocked, record it separately from implementation status. Report changed behavior, evidence, commit/branch and the next prompt. Never claim a gate passed unless it was run or explicitly verified as a document review.
