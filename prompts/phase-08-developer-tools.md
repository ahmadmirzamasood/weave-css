# Phase 08: Finish CLI, source maps and editor basics

Copy this file into Claude Code, or ask it to read and execute this file.

## Prompt

You are implementing Weave, an independent Rust CSS preprocessor. Read CLAUDE.md, instructions/AGENT.md, MEMORY.md, PRD.md, DEV-PLAN.md, SECURITY.md and the relevant design documents before editing. Inspect the current repository; do not assume memory proves code works.

### Prerequisites

Phase 07 gates pass.

### Work to perform

Complete build/check/watch, stdin/stdout, output path selection, root option, diagnostics in human/JSON form, dependency output, version/help and source maps. Use atomic output replacement and preserve last good output on failed watch builds. Handle file create/delete/rename and module graph changes. Prevent input/output collisions and watch loops on generated outputs. Implement source-map provenance and UTF-16 columns with Unicode and expansion tests. Add .weave syntax highlighting and sample configuration without claiming a full LSP.

### Acceptance gates

CLI integration tests cover exit codes, pipes, atomic writes, collisions and watch recovery; source-map decoded mappings point to expected original positions including Unicode; maps default to relative paths and exclude embedded sources unless opted in; documented examples compile.

### Scope boundaries

Do not publish an editor extension or registry package automatically. Document watch implementation and supported platforms.

### Handoff

Complete this phase only. Update MEMORY.md with actual state, decisions, exact validation commands/results, unresolved issues and next phase. Update relevant specs, changelog and the DEV-PLAN checklist. Commit and push the phase branch to the configured public repository; preserve unrelated work and remote history. If external publication is blocked, record it separately from implementation status. Report changed behavior, evidence, commit/branch and the next prompt. Never claim a gate passed unless it was run or explicitly verified as a document review.
