# Phase 11: Prepare the open-source release candidate

Copy this file into Claude Code, or ask it to read and execute this file.

## Prompt

You are implementing Weave, an independent Rust CSS preprocessor. Read CLAUDE.md, instructions/AGENT.md, MEMORY.md, PRD.md, DEV-PLAN.md, SECURITY.md and the relevant design documents before editing. Inspect the current repository; do not assume memory proves code works.

### Prerequisites

Phase 10 acceptance and evidence complete.

### Work to perform

Follow RELEASE.md. Polish public README/tutorial/API/CLI documentation, migration and compatibility policy, examples, changelog and contributor setup. Verify working name/repository/crate/binary collisions before registry naming. Prepare versioned packages, release CI with pinned tools/actions, artifact checksums and provenance. Run package dry runs and clean-environment installation/compilation for supported targets. Prepare release notes with exact supported features, benchmark links, known limits and security reporting route.

### Acceptance gates

Release candidate tied to an exact commit; all functional/security/performance gates evidenced; reproducible build and install instructions; package contents inspected; draft notes and rollback procedure ready; public GitHub repository contains complete source and MIT notices.

### Scope boundaries

Prepare only unless user explicitly requests an actual release and credentials are configured. Never publish or tag a release merely because this planning phase is requested.

### Handoff

Complete this phase only. Update MEMORY.md with actual state, decisions, exact validation commands/results, unresolved issues and next phase. Update relevant specs, changelog and the DEV-PLAN checklist. Commit and push the phase branch to the configured public repository; preserve unrelated work and remote history. If external publication is blocked, record it separately from implementation status. Report changed behavior, evidence, commit/branch and the next prompt. Never claim a gate passed unless it was run or explicitly verified as a document review.
