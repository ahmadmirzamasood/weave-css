# Changelog

## Unreleased

- Add project requirements, language proposal, Claude Code instructions, development phases, security and release policies.
- Publish planning package to https://github.com/ahmadmirzamasood/weave-css (phase 00).
- Freeze the Weave v0.1 language specification (phase 01): normative docs/LANGUAGE.md, docs/grammar.ebnf, docs/TYPES.md, docs/CSS-COMPATIBILITY.md; ADRs 0002–0008; 35 specification fixtures in tests/fixtures/spec (pending a compiler runner).
- Language changes from the proposal: `rule` takes a raw CSS selector prelude instead of a quoted string; statement keywords always start statements; `load` must precede other statements; CSS fragment literal `` `…` ``; `\#{` literal marker; parent lists with more than one selector lower to `:is()`.
- No compiler functionality is implemented yet.
