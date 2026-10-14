# ADR 0005: Interpolation contexts, escaping and injection policy

Status: accepted (phase 01, 2026-10-15).

## Context

Interpolation is the only bridge from compile-time values into CSS text. It must be impossible for a value to add declarations or rules, and the way to write a literal `#{` must be precise.

## Decision

- Five contexts — value, string, url, selector, declaration name — each with a fixed per-type serialization (LANGUAGE.md §7.1).
- Identifier escaping uses CSSOM rules and distinguishes identifier-start from continuation position.
- Injection policy: no interpolation can introduce `;`, `{`, `}`, unbalanced brackets, string terminators or comment openers. Fragments are the only way to add CSS tokens, and they are validated when created.
- `\#{` is the literal marker in every interpolating context. In strings it matches CSS semantics; outside strings it is a documented deviation (`\23 {` expresses the rare raw-CSS meaning). Opaque blocks, comments and unknown at-rules never interpolate.
- Unknown at-rules are fully verbatim (no interpolation), so future syntax is never misread.

## Consequences

Identifiers in at-rule preludes need `css.ident(name)` or a fragment, which is explicit but slightly more verbose. Escapes are deterministic and testable.
