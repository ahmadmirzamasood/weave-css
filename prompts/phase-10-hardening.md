# Phase 10: Harden and audit the release candidate

Copy this file into Claude Code, or ask it to read and execute this file.

## Prompt

You are implementing Weave, an independent Rust CSS preprocessor. Read CLAUDE.md, instructions/AGENT.md, MEMORY.md, PRD.md, DEV-PLAN.md, SECURITY.md and the relevant design documents before editing. Inspect the current repository; do not assume memory proves code works.

### Prerequisites

Phase 09 report and functional suite available.

### Work to perform

Run bounded fuzz campaigns for scanner/parser/compiler with seeded CSS and Weave corpora; minimize and fix crashes. Exercise every SECURITY.md limit and attack surface, dependency advisories/license/native-build review, path/symlink cases, control characters and source-map privacy. Test supported platform/toolchain matrix. Build a feature-to-fixture traceability report for every PRD requirement. Create docs/reports/security-review.md and qualification.md with exact evidence and remaining risks.

### Acceptance gates

No known input-induced panic/hang in exercised corpus; limits fail gracefully; all critical/high exploitable findings resolved or release blocked; fuzz duration/corpus recorded; dependency exceptions explicit; every requirement has evidence or an accurately labeled gap.

### Scope boundaries

Fuzzing is evidence, not proof. Do not mark unsupported features implemented just to complete the checklist.

### Handoff

Complete this phase only. Update MEMORY.md with actual state, decisions, exact validation commands/results, unresolved issues and next phase. Update relevant specs, changelog and the DEV-PLAN checklist. Commit and push the phase branch to the configured public repository; preserve unrelated work and remote history. If external publication is blocked, record it separately from implementation status. Report changed behavior, evidence, commit/branch and the next prompt. Never claim a gate passed unless it was run or explicitly verified as a document review.
