# Project memory

Updated: 2026-10-15 (Asia/Karachi).

## Confirmed user requirements

- New CSS preprocessor, implemented in Rust; independently designed syntax.
- Own file extension; compile to standard CSS.
- Variables, imports/modules, functions, maps, lists, reusable rules, and full CSS expressiveness.
- Performance is a primary goal; all code on GitHub in a public open-source repository.
- Development will be performed with Claude Code; documentation and phase prompts stay separate.

## Working decisions

- Working name Weave, extension `.weave`, repository `weave-css` created; language/package-name clearance still unverified.
- MIT license selected for the starter package.
- Rust dependencies permitted subject to dependency policy; no non-Rust runtime required.
- Explicit `#{expression}` interpolation in CSS values and selector templates.
- Language design is a proposal until phase 01; no existing preprocessor compatibility promise.

## Actual status

Planning documents and prompts prepared. No Rust compiler implemented, no tests run against compiler code, no performance claims.

Phase 00 complete (2026-10-15): public repository https://github.com/ahmadmirzamasood/weave-css, owner ahmadmirzamasood, default branch main. Initial commit 35a62bc2948a503465c1698470ed183785033e31 verified equal locally and via `git ls-remote origin refs/heads/main`; `gh repo view` reported visibility PUBLIC; GitHub detects license MIT.

Repository settings applied via gh API and verified: Issues enabled; private vulnerability reporting enabled (`{"enabled":true}`); ruleset `protect-main` (active, default branch) blocks deletion and force pushes. No required reviews or status checks yet, so sole-maintainer pushes work. Manual/later: add required CI status checks to the ruleset after phase 02; optionally enable Dependabot alerts and secret-scanning push protection in Settings → Code security.

## Next action

Run prompts/phase-01-language-spec.md on branch `phase/01-language-spec`.

## Session handoff template

Date / phase / branch / commit:
Implemented and verified:
Exact commands and results:
Decisions and linked ADRs:
Known failures or limitations:
Next task:

Keep this file concise. Move long evidence to docs/reports/. Never store credentials or private personal context here.
