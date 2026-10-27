# Weave CSS-subset fixtures

Fixtures for the plain-CSS subset of Weave implemented in phase 03: `rule` blocks at stylesheet level, declarations, known and unknown at-rules, `opaque` blocks, comments, literal markers and the errors around them, including explicit E5001 diagnostics for constructs this build does not implement. They are executed by [crates/weave-compiler/tests/fixtures.rs](../../../crates/weave-compiler/tests/fixtures.rs) (`cargo test`), and every case must pass.

## Layout

The layout and the `expected.diag` format are those of the [specification fixtures](../spec/README.md), with these differences:

- `case.toml` has `description`, `spec`, `expect` (`css` or `error`) and, for `expect = "css"`, `validation`. The entry is always `main.weave`.
- `validation = "independent"`: the output is also parsed by an independent CSS Syntax Level 3 parser (Servo's `cssparser`, a test-only dependency), which must report no rejected rule or declaration, bad string, bad URL, stray closing bracket, `{}` block in a non-custom property value or nested style rule. `validation = "preserve-only"`: the case preserves syntax that such a parser may reject (for example `*zoom` hacks in `opaque` blocks), so only the exact bytes are compared.
- Successful cases are compiled twice and must produce identical bytes.
- Error cases must produce exactly the listed diagnostics: no additional ones.

Expected outputs and positions were derived by hand from the specification before the runner was executed.

## Index

| Case | Expect | Covers |
|---|---|---|
| [001-rules-and-declarations](001-rules-and-declarations/) | css | Top-level `rule` blocks: one empty line between rules, empty rules omitted, duplicate declarations kept in order, `!important` in any case and spacing, empty values, whitespace around `:`, final declaration without `;`, ordinary comments dropped and `/*!` comments kept in place. |
| [002-values-strings-urls-comments](002-values-strings-urls-comments/) | css | Declaration values are CSS component values: `;`, `{`, `}` and `/*` inside strings, URLs and comments do not end them; data URLs, escapes, `!important` inside strings, unicode-range, hex colors in any case and CSS functions are emitted exactly; nothing is evaluated. |
| [003-custom-properties](003-custom-properties/) | css | Custom properties keep their value text exactly, including `{}` blocks, JSON-like content, strings with braces, odd tokens and case; leading and trailing whitespace is trimmed and `!important` is recognized. |
| [004-statement-at-rules](004-statement-at-rules/) | css | Statement at-rules: `@charset "UTF-8"` as the first item, `@import` and `@layer` statements before rules, `@namespace` after imports; preludes have comments removed and whitespace collapsed. |
| [005-group-at-rules](005-group-at-rules/) | css | Top-level group at-rules (`@media`, `@supports`, `@container`, `@layer` blocks, `@starting-style`) contain stylesheet items; nested group rules stay nested; empty group blocks are always emitted while empty style rules inside them are omitted; preludes are normalized. |
| [006-descriptor-at-rules](006-descriptor-at-rules/) | css | Declaration-list at-rules (`@font-face`, `@property`, `@counter-style`, `@font-palette-values`, `@view-transition`, `@position-try`) contain declarations; empty blocks are kept. |
| [007-keyframes](007-keyframes/) | css | `@keyframes` contains keyframe blocks with native selectors (`from`, `to`, percentages, lists); each block is a declaration list. Vendor-prefixed keyframes are unknown at-rules and stay verbatim. |
| [008-page-and-margins](008-page-and-margins/) | css | `@page` bodies hold declarations and the page-margin at-rules, whose bodies are declaration lists. |
| [009-scope](009-scope/) | css | `@scope` at stylesheet level contains style rules (use `:scope` rather than `&`). |
| [010-unknown-at-rules](010-unknown-at-rules/) | css | Unknown and vendor-prefixed at-rules are emitted as their exact source text in every context, with no interpolation and no comment removal; `}` and `;` inside strings, comments and URLs do not end them; `@font-feature-values` is preserved the same way. |
| [011-opaque-blocks](011-opaque-blocks/) | css (preserve-only) | `opaque` blocks are trimmed and emitted verbatim at stylesheet level and inside a rule's segment: hacks, `#{`, `\#{`, comments and unknown syntax survive; strings and comments containing `}` do not end the block; later lines keep their source indentation; empty `opaque` emits nothing. Validation is preservation-only: CSS parsers reject the hacks. |
| [012-unicode](012-unicode/) | css | Non-ASCII identifiers, strings, emoji and right-to-left text survive unchanged; a leading byte-order mark is not emitted; CSS escapes are preserved as written. |
| [013-literal-markers](013-literal-markers/) | css | `\#{` is a literal `#{` in declaration values, strings, URLs and at-rule preludes; comments, `opaque` blocks and unknown at-rules keep `\#{` unchanged. |
| [014-whitespace-and-layout](014-whitespace-and-layout/) | css | Values keep internal whitespace and line breaks; leading and trailing whitespace is trimmed; tabs are preserved inside values; empty items, stray `;` and dropped comments between rules add no output. |
| [015-selectors](015-selectors/) | css | Selector lists are normalized structurally: `, ` between selectors, one space around combinators and for descendants, no whitespace in attribute selectors except before a modifier, pseudo-class arguments normalized recursively; strings, escapes and namespaces are preserved. |
| [016-statements-are-keywords](016-statements-are-keywords/) | css | Statement keywords always start statements: `rule` followed directly by a selector; property names that merely contain a keyword, and keyword names written with a CSS escape, are declarations. |
| [020-error-unterminated-string](020-error-unterminated-string/) | error | A line break inside a string is E0002 at the string; the value then runs to the next `;` as in CSS, and later independent errors are still reported. |
| [021-error-unterminated-comment](021-error-unterminated-comment/) | error | An unterminated comment is E0003 at `/*`; the open rule block is also reported. |
| [022-error-unbalanced-brackets](022-error-unbalanced-brackets/) | error | Mismatched brackets in values are E0005: an unclosed `(` ended by the block's `}` (reported at the innermost open bracket) and a `]` that matches nothing; unclosed blocks are reported at their `{`. |
| [023-error-brace-in-value](023-error-brace-in-value/) | error | A `{` at depth 0 in a non-custom property value is E0111 (a forgotten `;` or a selector written without `rule`); recovery skips the block; custom properties may contain blocks. |
| [024-error-unsupported-weave](024-error-unsupported-weave/) | error | Weave constructs this build does not implement are E5001 at their keyword or `#{`, never silently dropped: bindings, control flow, recipes, functions, modules, exports, `fail`/`warn`, interpolation in values, strings, URLs, property names, selectors and preludes, nested rules and nested group at-rules. |
| [025-error-contexts](025-error-contexts/) | error | Context matrix errors: declarations at stylesheet level (E6001), `&` without a parent (E6003), `rule` in a declaration list (E6002), items not allowed in an at-rule context (E6005), `@scope` in a rule (E6006), top-level-only statements elsewhere (E0104), `yield` outside a function (E4013), `slot` outside a recipe (E4012). |
| [026-error-charset-not-first](026-error-charset-not-first/) | error | `@charset` must be the first item of the entry module. |
| [027-error-charset-encoding](027-error-charset-encoding/) | error | `@charset` accepts only "UTF-8". |
| [028-error-import-order](028-error-import-order/) | error | Native `@import` after a style rule and `@namespace` after a style rule are E7010; Weave never reorders them. |
| [029-error-names-and-hacks](029-error-names-and-hacks/) | error | A declaration named by a remaining keyword is E0103 (escape it); a star hack is not a declaration name (E0101, use `opaque`); a raw selector block is E0110. |
| [030-error-malformed-rules](030-error-malformed-rules/) | error | Malformed `rule` and at-rule shapes: a missing selector, a missing block, a block at-rule without a block, a statement at-rule followed by a block, and an unmatched `}` at top level. |
| [031-error-nesting-depth](031-error-nesting-depth/) | error | Brackets nested deeper than the default nesting-depth limit (128, blocks and brackets combined) are E8002 and stop parsing without a crash. |
| [032-error-unicode-positions](032-error-unicode-positions/) | error | Diagnostic columns count Unicode scalar values, not bytes or UTF-16 code units. |
| [033-error-unterminated-interpolation](033-error-unterminated-interpolation/) | error | An interpolation without a closing `}` is E0004 at `#{`; it runs to the end of input, so the rule block is unclosed too. |
