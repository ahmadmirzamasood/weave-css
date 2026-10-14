# Phase 00: Public GitHub bootstrap

Copy this file into Claude Code, or ask it to read and execute this file.

## Prompt

You are implementing Weave, an independent Rust CSS preprocessor. Read CLAUDE.md, instructions/AGENT.md, MEMORY.md, PRD.md, DEV-PLAN.md, SECURITY.md and the relevant design documents before editing. Inspect the current repository; do not assume memory proves code works.

### Prerequisites

None; use the extracted planning package.

### Work to perform

Inspect git state and preserve existing content. Verify GitHub CLI availability and authentication without printing tokens. Choose the authenticated personal account unless the user specified an organization. Check whether weave-css exists under that account. Never overwrite an unrelated repository; if the name is occupied, select weave-css-rust if free and record it, or ask if ownership is ambiguous. Keep Weave branding provisional.
Initialize a standalone git repository in this package directory if needed, default branch main. Review files for secrets, commit the planning kit, then create a public repository from the local source and push. Verify returned repository visibility is PUBLIC and the pushed head matches the local commit. Record the actual URL in README and MEMORY and push that update.
Enable issues and private vulnerability reporting if supported by permissions; record any manual settings required. Configure main branch protection/rulesets when supported without blocking sole-maintainer bootstrap. Do not claim settings succeeded if API permissions prevent them.

### Acceptance gates

Public repository URL and verified PUBLIC visibility; exact pushed SHA; all planning files present remotely; MIT license present; actual settings recorded. If auth or creation is unavailable, retain the complete local work and report the precise blocker without marking publication complete.

### Scope boundaries

Do not implement compiler features. Use docs/GITHUB-SETUP.md for commands. No release or registry publishing.

### Handoff

Complete this phase only. Update MEMORY.md with actual state, decisions, exact validation commands/results, unresolved issues and next phase. Update relevant specs, changelog and the DEV-PLAN checklist. Commit and push the phase branch to the configured public repository; preserve unrelated work and remote history. If external publication is blocked, record it separately from implementation status. Report changed behavior, evidence, commit/branch and the next prompt. Never claim a gate passed unless it was run or explicitly verified as a document review.
