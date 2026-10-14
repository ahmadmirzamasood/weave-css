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
- Weave v0.1 language is frozen (phase 01) in docs/LANGUAGE.md, docs/grammar.ebnf, docs/TYPES.md, docs/CSS-COMPATIBILITY.md; rationale in ADRs 0002–0008. No existing preprocessor compatibility promise.
- Key v0.1 decisions: every compile-time construct starts with a statement keyword (statement keywords always win over declaration detection); `rule <raw CSS selector> { }` at every level (quoted selectors dropped); `load` first in a module; binary64 numbers with six-decimal quantization and strict units; types number/string/color/bool/list/map/css-fragment, no null; backtick fragments are the only way to produce CSS tokens; strings always escaped per context; `\#{` literal marker; nesting lowered with direct substitution only when splice-safe, otherwise `:is(parent list)`; interleaved declarations become segments; post-order first-encounter module emission; native @import never reordered (E7010); LF output.
- Pinned references: CSS Syntax 3 CRD 2026-10-01, CSS Nesting 1 WD 2026-01-22, ECMA-426 1st edition (Dec 2024).

## Actual status

Planning documents and prompts prepared. No Rust compiler implemented, no tests run against compiler code, no performance claims.

Phase 00 complete (2026-10-15): public repository https://github.com/ahmadmirzamasood/weave-css, owner ahmadmirzamasood, default branch main. Initial commit 35a62bc2948a503465c1698470ed183785033e31 verified equal locally and via `git ls-remote origin refs/heads/main`; `gh repo view` reported visibility PUBLIC; GitHub detects license MIT.

Repository settings applied via gh API and verified: Issues enabled; private vulnerability reporting enabled (`{"enabled":true}`); ruleset `protect-main` (active, default branch) blocks deletion and force pushes. No required reviews or status checks yet, so sole-maintainer pushes work. Manual/later: add required CI status checks to the ruleset after phase 02; optionally enable Dependabot alerts and secret-scanning push protection in Settings → Code security.

Phase 01 complete as a document review (2026-10-15), branch `phase/01-language-spec`. Delivered: normative spec docs, grammar, types, CSS compatibility matrix, ADRs 0002–0008, PRD traceability, 35 specification fixtures in tests/fixtures/spec (18 success, 17 error) with case.toml/expected.css/expected.diag format. Validation performed: expected outputs derived and reviewed by hand against the spec; a generator script asserted every expected.diag position points at the stated token. No fixture has been executed; no compiler exists. `.gitattributes` keeps fixtures LF except the deliberate CRLF fixture 035.

Open (non-blocking for phases 02/03): independent human review of fixture expectations; equal-specificity expansion optimization (deferred, ADR needed); `@scope` nested in rules and `@font-feature-values` internals (verbatim only); full built-in reference due in phase 05 (docs/BUILTINS.md, changes need ADR); source-map primary-origin policy (phase 08); computed-style verification of fixture 021 (phase 06); language/package name clearance.

## Next action

Run prompts/phase-02-rust-foundation.md on branch `phase/02-rust-foundation` (merge or base on phase 01 first).

## Session handoff template

Date / phase / branch / commit:
Implemented and verified:
Exact commands and results:
Decisions and linked ADRs:
Known failures or limitations:
Next task:

Keep this file concise. Move long evidence to docs/reports/. Never store credentials or private personal context here.
