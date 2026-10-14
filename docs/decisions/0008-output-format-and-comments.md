# ADR 0008: Output format, comments and line endings

Status: accepted (phase 01, 2026-10-15).

## Decision

- One output style in v0.1: two-space indentation, one blank line between top-level items, `, `-joined selector lists, normalized selector and prelude whitespace, values and custom properties verbatim (trimmed).
- LF line endings everywhere, including verbatim regions (CSS preprocessing makes CR/CRLF/FF equivalent), so Windows and Unix checkouts produce identical bytes.
- Comments between items are dropped except `/*! … */`; comments inside values are kept; comments inside preludes are removed.
- Empty lowered group blocks are omitted; `@layer` blocks and literally written blocks are kept.
- No `@charset` is added; output is UTF-8 without BOM.

## Consequences

Fixture expectations compare bytes for formatting, plus semantic checks where order or specificity matters. A compressed style can be added later as an option without changing the default.
