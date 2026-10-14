# ADR 0002: Explicit statements and the `rule` keyword

Status: accepted (phase 01, 2026-10-15). Supersedes the quoted-selector form `rule ".card"` from the phase 00 proposal.

## Context

CSS cannot decide `a:hover { … }` versus a declaration `a: hover …` without unbounded lookahead, and languages that allow raw nested selectors resolve that heuristically. The proposal used `rule ".card"` with a quoted selector, but quoting creates two escape layers: the delimiting quote must be escaped, and CSS selector escapes (`.a\:b`) and attribute strings (`[title="x"]`) interact badly with string escapes.

## Decision

- Every compile-time construct begins with a statement keyword or is `#{…}`. Style rules are always introduced by `rule`, at every level including top level.
- The selector after `rule` is raw CSS up to the first depth-0 `{`, tokenized as CSS with interpolation in the selector context: `rule .card:hover { }`, `rule a[title="x"] { }`.
- Item classification is deterministic: a statement keyword always starts its statement (so `rule :root { }` is a rule, and nested `rule :hover` means `& :hover`, as in CSS Nesting); otherwise `DECL_NAME :` is a declaration; unprefixed selectors are E0110 with a hint. Properties named like a keyword need a CSS escape (`\72 ule`).
- No line comments; `//` remains CSS.

## Consequences

Plain CSS files are not valid Weave without adding `rule` (or wrapping in `opaque { }`); accepted because compatibility with existing syntax is a non-goal. Parsing needs one token of lookahead after a name. A future CSS property named like a Weave keyword would need a CSS escape; none exists today.
