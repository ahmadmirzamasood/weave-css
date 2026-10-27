# Weave v0.1 specification fixtures

Status: **specification fixtures, executed by the fixture runner** ([crates/weave-compiler/tests/fixtures.rs](../../../crates/weave-compiler/tests/fixtures.rs), run by `cargo test`). Expected outputs were derived by hand from [LANGUAGE.md](../../../docs/LANGUAGE.md), [TYPES.md](../../../docs/TYPES.md) and [CSS-COMPATIBILITY.md](../../../docs/CSS-COMPATIBILITY.md) and reviewed as documents. The runner classifies each case as *pass*, *pending* (the build reports E5001 for a construct it does not implement yet) or *fail*; no case may fail, and the list of passing cases is pinned in the runner. Phase 03 (plain-CSS subset) passes 001, 024, 029 and 033; the other 31 are pending. When the runner disagrees with a fixture, decide whether the fixture or the specification is wrong; do not silently update expectations.

## Layout

Each directory is one case:

- `case.toml`: `description`, `spec` (sections exercised), `status` (always `specification`), `entry` (always `main.weave`), `expect` (`css` or `error`).
- `*.weave`: the entry module and any modules it loads (paths relative to the case directory, which is the module root).
- `expected.css`: exact expected bytes for successful builds (LF line endings, UTF-8, trailing newline).
- `expected.diag`: for failing builds. One diagnostic per line: `severity code file:line:column "message substring"`. Lines starting with `#` are comments. Line and column are 1-based; columns count Unicode scalar values. A runner must match severity, code, file and start position exactly, and require the message (or its notes) to contain the substring case-insensitively. Listed diagnostics are the required primary diagnostics; additional diagnostics are allowed only if they are independent errors.

Position conventions: binary-operator errors point at the operator; name errors at the name; argument errors at the argument (name for named arguments); built-in failures at the callee; interpolation type errors at `#{`; statement-level errors at the statement keyword or first token of the item.

Semantic checks: fixture 021 lists expected specificities; the nesting implementation must verify them with computed-style tests, not only by comparing text.

`035-unicode-and-crlf/main.weave` intentionally uses CRLF line endings; `.gitattributes` keeps it byte-exact.

## Index

| Case | Expect | Covers |
|---|---|---|
| [001-plain-css-preservation](001-plain-css-preservation/) | css | Plain CSS inside `rule`: selector normalization, verbatim values with strings/URLs/comments containing `;` and braces, custom properties with blocks and empty values, `!important` normalization, comment policy, final declaration without `;`. |
| [002-bindings-value-interpolation](002-bindings-value-interpolation/) | css | Immutable bindings interpolated in the value context: numbers, colors (normalized lowercase hex), quoted strings, fragments, booleans, expressions inside CSS functions; a bare identifier in a value stays CSS. |
| [003-lexical-scope-shadowing](003-lexical-scope-shadowing/) | css | Block scopes: shadowing in rules, nested rules and loop iterations; the module binding is unchanged after the blocks; interleaved declarations produce separate segments. |
| [004-error-duplicate-binding](004-error-duplicate-binding/) | error | Two bindings of the same name in one scope are rejected; the diagnostic points at the second name. |
| [005-error-unresolved-name](005-error-unresolved-name/) | error | A bare identifier in a CSS value is literal, but the same identifier inside interpolation must resolve to a binding. |
| [006-numbers-units-precision](006-numbers-units-precision/) | css | Unit conversion (right operand converts to the left unit), six-decimal quantization, remainder sign, quantized equality, unit equality is never implicit, negative zero, large integers without exponent, unit case canonicalization. |
| [007-error-incompatible-units](007-error-incompatible-units/) | error | Adding an absolute length and a relative length is rejected; the diagnostic points at the operator. |
| [008-precedence-short-circuit](008-precedence-short-circuit/) | css | Operator precedence and left associativity; `&&`/`||` short-circuit so the skipped operands (division by zero, out-of-range index) are never evaluated; string concatenation and equality. |
| [009-error-division-by-zero](009-error-division-by-zero/) | error | Division by a zero dimension is an error, reported at the operator. |
| [010-strings-contexts-escaping](010-strings-contexts-escaping/) | css | String serialization in the value, string and url contexts; CSS-style string escapes; control characters; literal `\#{` markers in strings and bare values. |
| [011-maps-lists-sequences](011-maps-lists-sequences/) | css | Map dot and bracket access, list indexing, insertion-ordered map iteration, indexed list iteration, explicit css.space/css.comma/css.slash sequences, map.get default, selector-context key interpolation. |
| [012-error-list-interpolation](012-error-list-interpolation/) | error | Lists never serialize implicitly; interpolating one directly requires css.space or css.comma. Reported at `#{`. |
| [013-error-index-out-of-range](013-error-index-out-of-range/) | error | Zero-based list indexing with a bounds check; index 2 of a two-element list is an error reported at `[`. |
| [014-functions-defaults-named-recursion](014-functions-defaults-named-recursion/) | css | Typed functions with defaults referring to module bindings and earlier parameters, named and reordered arguments, early `yield` inside `when`, recursion, hoisting (a function used before its definition). |
| [015-error-unknown-named-argument](015-error-unknown-named-argument/) | error | Unknown named arguments are rejected (no silent ignoring); reported at the argument name. |
| [016-recipes-content-blocks](016-recipes-content-blocks/) | css | Recipes with defaults and optional argument lists; recipe hygiene (recipe body sees its definition scope, content block sees the caller scope); slot inside a nested rule; segment order around recipe output. |
| [017-error-content-block-not-placed](017-error-content-block-not-placed/) | error | A content block passed to a recipe that never executes `slot` is an error, reported at `apply`. |
| [018-conditions-ranges-selector-escaping](018-conditions-ranges-selector-escaping/) | css | `when`/`otherwise when`/`otherwise` chains, `range` with positive and negative steps, `rule :root` as a statement, selector-context identifier escaping at start and continuation positions (`.w-0\.5`, `.\31 up`). |
| [019-error-non-boolean-condition](019-error-non-boolean-condition/) | error | No implicit truthiness: a number used as a condition is rejected at the condition expression. |
| [020-nesting-parent-references](020-nesting-parent-references/) | css | Explicit `&`, implicit descendant, leading combinators, `&` inside `:not()` (not relative), `&` in a later compound, selector lists with `&` and attribute strings, interleaved declarations. |
| [021-nesting-specificity-is](021-nesting-specificity-is/) | css | Specificity-preserving lowering: multi-selector parents use :is() (expansion would give `.side .item` specificity (0,2,0) instead of native (1,1,0)); complex parent in a non-leading position uses :is() to keep matching; splice-safe cases substitute directly. Expected specificities: `:is(#main, .side) .item` (1,1,0); `.a .b.on` (0,3,0); `.x :is(.a .b)` (0,3,0); `a:is(span.x)` (0,1,2); `:where(.btn)` (0,0,0). Phase 06 must confirm with computed-style tests. |
| [022-nested-group-at-rules](022-nested-group-at-rules/) | css | @media/@supports/@container/@layer/@starting-style nested in a rule are lowered by wrapping; nested @media preludes are not merged; an empty nested @media is omitted; empty @layer blocks are kept. |
| [023-at-rule-body-contexts](023-at-rule-body-contexts/) | css | Declaration-list at-rules (@font-face, @property, @counter-style), keyframe lists with native selectors and `each`-generated percentage steps, identifier prelude via css.ident, @page with a margin at-rule, non-ASCII string preserved. |
| [024-opaque-and-unknown-syntax](024-opaque-and-unknown-syntax/) | css | Unknown at-rules are emitted as exact source text with no interpolation; `opaque` blocks are trimmed and verbatim (hacks and literal `#{`); unknown at-rules and opaque blocks inside a rule stay inside its segment; custom properties keep JSON-like braces and strings containing `}`. |
| [025-modules-diamond-namespaces](025-modules-diamond-namespaces/) | css | Multi-module build: namespaced bindings, functions and recipes; a diamond (main -> a -> c, main -> b -> c, main -> c) evaluates and emits c once; emission order c, a, b, main; recipe hygiene across modules; aliases differ per importer. |
| [026-error-module-cycle](026-error-module-cycle/) | error | A load cycle is rejected at the load that closes it, with the full load chain (main.weave, a.weave, b.weave, a.weave) in the message or notes. |
| [027-error-member-not-exported](027-error-member-not-exported/) | error | Module members are private unless exported; access through the namespace is rejected at the member name. |
| [028-error-native-import-order](028-error-native-import-order/) | error | Native @import is never reordered: after module concatenation it would follow a dependency's style rule, so the build fails at the @import. |
| [029-error-selector-without-rule](029-error-selector-without-rule/) | error | Qualified rules must be introduced by `rule`; a raw selector block is rejected with a hint instead of being guessed. |
| [030-interpolation-injection-safety](030-interpolation-injection-safety/) | css | Hostile strings cannot add declarations or rules: quoted in values, escaped inside CSS strings, identifier-escaped in property names and selectors. |
| [031-error-invalid-fragment](031-error-invalid-fragment/) | error | css.fragment validates runtime text: a top-level `;` would allow declaration injection, so it is rejected at the call. |
| [032-error-argument-type](032-error-argument-type/) | error | Parameter type annotations are checked at the call: a unitless number is not a length. |
| [033-error-top-level-declaration](033-error-top-level-declaration/) | error | Declarations are not allowed at stylesheet level. |
| [034-error-nesting-under-pseudo-element](034-error-nesting-under-pseudo-element/) | error | `&` cannot represent pseudo-elements, so a nested rule under `.a::before` is rejected at `rule`. |
| [035-unicode-and-crlf](035-unicode-and-crlf/) | css | CRLF source (kept by .gitattributes) produces LF output; non-ASCII identifiers, strings and selector interpolation are preserved; a multi-line value keeps its internal whitespace. |
