# ADR 0006: Nesting lowering preserves CSS Nesting specificity

Status: accepted (phase 01, 2026-10-15). Reference: CSS Nesting Module Level 1, W3C Working Draft 22 January 2026.

## Context

Weave lowers nesting to flat CSS. Native nesting defines `&` like `:is(<parent list>)`, which takes the maximum specificity of the list. Naive Cartesian expansion changes specificity for mixed lists, changes matching when a complex parent is spliced into a non-leading position (`.x &` with parent `.a .b`), and grows exponentially.

## Decision

- Substitute the parent directly only when provably equivalent (a single parent selector in a splice-safe position, CSS-COMPATIBILITY.md §4.1); otherwise use `:is(parent list)`. Parent lists with more than one selector always use `:is()` in v0.1.
- Nested rules under pseudo-element selectors are errors (E6004).
- Interleaved declarations become separate rule segments in source order, matching the cascade position of `CSSNestedDeclarations`.
- Nested group at-rules are lowered by wrapping; nested `@media` preludes are never merged.
- `@scope` nested in rules is unsupported in v0.1 (E6006).

## Consequences

Output relies on `:is()` for multi-selector parents. Output is linear in input size. Phase 06 must verify specificity with computed-style tests, not only snapshots. An equal-specificity expansion optimization may be proposed later.
