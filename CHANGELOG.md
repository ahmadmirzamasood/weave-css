# Changelog

## Unreleased

- Publish the repository with license, security policy and contribution guide.
- Add the Weave v0.1 language specification: docs/LANGUAGE.md, docs/grammar.ebnf, docs/TYPES.md, docs/CSS-COMPATIBILITY.md; 35 specification fixtures in tests/fixtures/spec (pending a compiler runner).
- Notable v0.1 choices: `rule` takes a raw CSS selector prelude; statement keywords always start statements; `load` must precede other statements; CSS fragment literal `` `…` ``; `\#{` literal marker; parent lists with more than one selector lower to `:is()`.
- No compiler functionality is implemented yet.
