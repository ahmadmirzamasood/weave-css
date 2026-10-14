# ADR 0004: Value types, CSS fragments and no implicit serialization

Status: accepted (phase 01, 2026-10-15).

## Context

Stylesheet languages often blur strings, identifiers and raw CSS, making interpolation both surprising (quotes appearing or vanishing) and unsafe (data becoming syntax). The proposal also required that collections not silently become CSS.

## Decision

- Types: number, string, color (8-bit sRGB), bool, list, map (insertion-ordered, string keys) and `css` fragment. No null.
- Strings are data: they always serialize as quoted strings in values and as escaped identifiers in selectors and property names.
- CSS syntax is produced only by fragments: the backtick literal `` `…` `` or `css.ident`, `css.fragment`, `css.space`, `css.comma`, `css.slash`. Fragments are validated (balanced brackets, no braces, no top-level `;`).
- Lists and maps never serialize implicitly (E2004).
- Colors are hex literals only; other color syntaxes are fragments.
- Map equality ignores order; iteration preserves insertion order.

## Consequences

Identifier-valued variables are written as fragments (`` bind mode = `grid`; ``) or converted with `css.ident`. The type system is small enough for phase 04 and leaves room for richer color types later without changing existing semantics.
