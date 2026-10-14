# Project memory

Updated: 2026-10-15 (Asia/Karachi).

## Confirmed user requirements

- New CSS preprocessor, implemented in Rust; independently designed syntax.
- Own file extension; compile to standard CSS.
- Variables, imports/modules, functions, maps, lists, reusable rules, and full CSS expressiveness.
- Performance is a primary goal; all code on GitHub in a public open-source repository.
- Development will be performed with Claude Code; documentation and phase prompts stay separate.

## Working decisions

- Working name Weave, extension `.weave`, repository name `weave-css`; availability unverified.
- MIT license selected for the starter package.
- Rust dependencies permitted subject to dependency policy; no non-Rust runtime required.
- Explicit `#{expression}` interpolation in CSS values and selector templates.
- Language design is a proposal until phase 01; no existing preprocessor compatibility promise.

## Actual status

Planning documents and prompts prepared. No Rust compiler implemented, no tests run against compiler code, no performance claims. GitHub repository not created: available connector lacks repository creation and local GitHub CLI is unavailable in the preparation environment.

## Next action

Run prompts/phase-00-github.md in an authenticated development environment, then phase 01.

## Session handoff template

Date / phase / branch / commit:
Implemented and verified:
Exact commands and results:
Decisions and linked ADRs:
Known failures or limitations:
Next task:

Keep this file concise. Move long evidence to docs/reports/. Never store credentials or private personal context here.
