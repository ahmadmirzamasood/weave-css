# Weave v0.1 CSS compatibility contract

Status: normative companion to [LANGUAGE.md](LANGUAGE.md).

Weave emits standard CSS. It does not validate property values, add vendor prefixes, polyfill features or minify. Browser support depends entirely on the emitted CSS.

## 1. Reference specifications

Verified on 2026-10-15. Later revisions are adopted only as explicit specification changes.

| Subject | Specification | Revision used |
|---|---|---|
| Tokenization, component values, preprocessing | CSS Syntax Module Level 3 | W3C Candidate Recommendation Draft, 1 October 2026 — https://www.w3.org/TR/2026/CRD-css-syntax-3-20261001/ |
| Nesting, `&`, specificity, nested declarations | CSS Nesting Module Level 1 | W3C Working Draft, 22 January 2026 — https://www.w3.org/TR/2026/WD-css-nesting-1-20260122/ |
| Source maps | ECMA-426 Source map format | 1st edition, December 2024 (version field `3`, 0-based columns in UTF-16 code units for CSS) — https://ecma-international.org/publications-and-standards/standards/ecma-426/ |
| Selectors, `:is()` specificity | Selectors Level 4 | as referenced by CSS Nesting above |
| Identifier/string serialization | CSSOM *serialize an identifier* / *serialize a string* | as referenced by CSS Syntax above |

## 2. Context matrix

✓ allowed · ✗ error (code) · V verbatim (no Weave processing inside)

| Item | stylesheet | rule body | declaration list | keyframe list | page body | function body |
|---|---|---|---|---|---|---|
| declaration | ✗ E6001 | ✓ | ✓ | ✗ E6005 | ✓ | ✗ E4014 |
| `rule` | ✓ | ✓ (nested) | ✗ E6002 | ✗ E6005 | ✗ E6005 | ✗ E4014 |
| keyframe block (`from`, `to`, `N%`) | ✗ E0110 | ✗ E0110 | ✗ E0101 | ✓ | ✗ E0101 | ✗ E4014 |
| `bind`, `when`, `each`, `fail`, `warn` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ |
| `apply` | ✓ (expansion must not yield declarations, E6001) | ✓ | ✓ (expansion must be declarations only, E6002) | ✗ E6005 | ✓ (declarations only) | ✗ E4014 |
| `slot` | in recipe bodies only (E4012) | in recipe bodies only | ✗ E6005 | ✗ E6005 | ✗ E6005 | ✗ E4014 |
| `yield` | ✗ E4013 | ✗ E4013 | ✗ E4013 | ✗ E4013 | ✗ E4013 | ✓ |
| `load`, `export`, `fn`, `recipe` | top level only | ✗ E0104 | ✗ E0104 | ✗ E0104 | ✗ E0104 | ✗ E0104 |
| `opaque { }` | ✓ V | ✓ V | ✗ E6005 | ✗ E6005 | ✗ E6005 | ✗ E4014 |
| nested group at-rule (`@media`, `@supports`, `@container`, `@layer {}`, `@starting-style`) | ✓ | ✓ (lowered) | ✗ E6005 | ✗ E6005 | ✗ E6005 | ✗ E4014 |
| declaration-list at-rules, `@keyframes`, `@page`, `@scope`, `@import`, `@namespace`, `@layer a, b;` | ✓ | ✗ E6005 (`@scope`: E6006) | ✗ E6005 | ✗ E6005 | ✗ E6005 | ✗ E4014 |
| page-margin at-rules (`@top-left`, …) | ✗ E6005 | ✗ E6005 | ✗ E6005 | ✗ E6005 | ✓ | ✗ E4014 |
| unknown at-rule | ✓ V | ✓ V (emitted inside the current rule segment) | ✓ V | ✓ V | ✓ V | ✗ E4014 |
| `/*! … */` comment | ✓ V | ✓ V | ✓ V | ✓ V | ✓ V | dropped |

Inside a group at-rule the body context is the enclosing context: stylesheet at top level, rule body when nested in a rule.

## 3. At-rule matrix

At-rule names match ASCII case-insensitively and are emitted as written. Vendor-prefixed names (`@-webkit-keyframes`) are unknown at-rules.

| At-rule | Kind | Prelude | Body context | Where allowed | Notes |
|---|---|---|---|---|---|
| `@charset` | statement | `"UTF-8"` only (exactly that string), no interpolation | — | first item of the entry module (only whitespace and dropped comments may precede it) | emitted first; anything else E6008 |
| `@import` | statement | value context | — | module top level (inside a group at-rule: E6005) | ordering checked (§5), never resolved or inlined |
| `@namespace` | statement | value context | — | module top level (inside a group at-rule: E6005) | ordering checked (§5) |
| `@layer` (statement) | statement | value context | — | stylesheet | name list |
| `@layer` (block) | group | value context | enclosing | stylesheet, rule body | always emitted, even if empty |
| `@media` | group | value context | enclosing | stylesheet, rule body | nested `@media` stays nested; preludes are never merged |
| `@supports` | group | value context | enclosing | stylesheet, rule body | |
| `@container` | group | value context | enclosing | stylesheet, rule body | |
| `@starting-style` | group | none | enclosing | stylesheet, rule body | |
| `@scope` | group | value context | stylesheet (rules only) | stylesheet | nested in a rule: E6006 in v0.1; declarations directly in the body: E6001; `&` inside: E6003 (use `:scope`) |
| `@font-face` | descriptors | none | declaration list | stylesheet | |
| `@font-palette-values` | descriptors | value context | declaration list | stylesheet | |
| `@counter-style` | descriptors | value context | declaration list | stylesheet | |
| `@property` | descriptors | value context | declaration list | stylesheet | |
| `@view-transition` | descriptors | none | declaration list | stylesheet | |
| `@position-try` | descriptors | value context | declaration list | stylesheet | |
| `@keyframes` | keyframes | value context | keyframe list; each keyframe block is a declaration list | stylesheet | keyframe selectors are native (`from`, `to`, percentages) and may interpolate |
| `@page` | page | value context | page body; margin at-rules are declaration lists | stylesheet | 16 margin at-rule names per CSS Paged Media |
| `@font-feature-values` | known, preserved | verbatim | verbatim | stylesheet | sub-at-rules have a special grammar; preserved like an unknown at-rule |
| any other | unknown | verbatim | verbatim | everywhere except function bodies | no interpolation, no statements; `#{` preserved |

“Value context” preludes accept `#{…}` with value-context serialization. Strings serialize quoted, so identifier positions (layer, keyframes, container and counter-style names) need `css.ident(…)` or a fragment.

## 4. Nesting lowering and specificity

Weave lowers all nesting to flat CSS whose matching and specificity equal those of CSS Nesting Level 1 (WD 2026-01-22), in which `&` behaves like `:is(<parent selector list>)` and a relative nested selector gets the specificity of that implied `&`.

### 4.1 Algorithm (normative)

Input: the enclosing rule's lowered selector list `P = [p1 … pn]` (no `&` remains) and one complex selector `C` from the nested `rule` prelude. Each item of a nested selector list is lowered independently; output keeps the child list order.

1. **Relative selectors.** If `C` contains no `&` anywhere (including inside pseudo-class arguments): if `C` begins with a combinator, prepend `&`; otherwise prepend `& ` (descendant).
2. **Pseudo-elements.** If any `pi` contains a pseudo-element, a nested `rule` is E6004. (Nested group at-rules and declarations are still fine; they reuse `P` unchanged.)
3. **Compound validity.** A type or universal selector that is not the first simple selector of its compound in `C` (counting `&` as a simple selector, as in `&div`) is E6007.
4. **Replace each `&` occurrence `o`:**
   - If `n = 1` and the occurrence is *splice-safe* for `p1`, replace it with `p1`.
   - Otherwise replace it with `:is(p1, …, pn)`.
5. An occurrence is **splice-safe** for `p` when either:
   - (a) `o` is not inside a pseudo-class argument and `o` is the first simple selector of the first compound of `C` (then `p` may be a complex selector: `p` followed by the rest of `C` is equivalent to `:is(p)` followed by the rest); or
   - (b) `p` is a single compound selector and `o` is the first simple selector of its compound or `p` has no type/universal selector.
6. Count emitted complex selectors against the selector-expansion limit (E8005).

Weave never performs Cartesian expansion of parent lists in v0.1; parent lists with more than one selector always produce `:is(…)`. This is always equivalent to native nesting and keeps output linear in size. Expansion of equal-specificity lists may be added later as an output optimization.

### 4.2 Examples (normative)

| Parent | Nested `rule` | Lowered selector | Why |
|---|---|---|---|
| `.card` | `&:hover` | `.card:hover` | (a) |
| `.card` | `.title` | `.card .title` | implicit `& `, then (a) |
| `ul li` | `> a` | `ul li > a` | implicit `&`, then (a) |
| `.a .b` | `&.on` | `.a .b.on` | (a): `p1` complex, `&` leads |
| `.a .b` | `.x &` | `.x :is(.a .b)` | not splice-safe: `.x .a .b` would require `.x` to be an ancestor of `.a` |
| `.btn` | `.group:hover &` | `.group:hover .btn` | (b) |
| `.btn` | `:not(&)` | `:not(.btn)` | contains `&`, so not relative; (b) |
| `.a` | `& + &` | `.a + .a` | (a) for the first, (b) for the second |
| `span.x` | `a&` | `a:is(span.x)` | (b) fails: `p1` has a type selector and `&` is not first |
| `#main, .side` | `.item` | `:is(#main, .side) .item` | `n > 1`; specificity (1,1,0) for every match, as native nesting. Expanding to `#main .item, .side .item` would give `.side .item` only (0,2,0) and change the cascade |
| `.a, .b` | `&:hover` | `:is(.a, .b):hover` | `n > 1` |
| `.a::before` | `&:hover` | E6004 | `&` cannot represent pseudo-elements |
| (none) | `& .x` | E6003 | no parent rule |
| `.a` | `&div` | E6007 | type selector must start a compound |

### 4.3 Interleaved declarations

Declarations keep source order relative to nested rules and nested group at-rules. Each maximal run of declarations is a separate rule segment with the rule's lowered selector, emitted at its position. This reproduces the cascade position that CSS Nesting gives to `CSSNestedDeclarations`.

```weave
rule .a {
    color: red;
    rule .b { color: blue; }
    @media (width > 40em) { color: green; }
    margin: 0;
}
```

```css
.a {
  color: red;
}

.a .b {
  color: blue;
}

@media (width > 40em) {
  .a {
    color: green;
  }
}

.a {
  margin: 0;
}
```

A nested group at-rule inside a rule wraps whatever its body produces: declarations become a segment with the current selector; nested rules are lowered against the current selector.

## 5. Module emission and CSS ordering

### 5.1 Order

`load` statements are traversed depth first in source order; each module's CSS follows the CSS of the modules it loads, and each module is emitted once, at its first encounter. For

```
main.weave: load "./a.weave"; load "./b.weave";
a.weave:    load "./c.weave";
b.weave:    load "./c.weave";
```

the output order is `c`, `a`, `b`, `main`. Every module's top-level output is emitted even if the importer uses only its exports. Modules with only definitions emit nothing.

### 5.2 Statement at-rules

CSS requires `@charset` first, then `@import` (optionally interleaved with `@layer` statements), then `@namespace`, then everything else. Weave never reorders cascade-relevant content to satisfy this. After module concatenation:

- `@charset` may appear only as the first item of the entry module (E6008 otherwise) and is then emitted first.
- If an `@import` or `@namespace` follows any item other than `@charset`, `@layer` statements, `@import`, `@namespace` (for `@namespace`) or `/*!` comments, the build fails with E7010 at the late at-rule, with a note naming the earlier item and its module.

Typical fix: move native `@import` statements into the first module loaded, or into the entry module before any rules when no dependency emits CSS.

## 6. Declarations and values

| Construct | Behavior |
|---|---|
| Standard and unknown properties | name and value emitted as written (whitespace trimmed at ends) |
| Custom properties `--x` | value token text preserved exactly, including `{}` blocks, comments and odd tokens; empty value emitted as `--x:;`; name case preserved |
| `!important` | recognized at depth 0 at the end; emitted as ` !important` |
| `var()`, `env()`, `calc()`, `min()`, `clamp()`, `attr()`, unknown functions | literal CSS, never evaluated; interpolation inside them is allowed |
| `/` in values | CSS syntax, never division (division only exists inside expressions) |
| Strings and `url(…)` containing `;`, `{`, `}`, `/*` | preserved; they do not end the declaration |
| `data:` URLs, escapes, `unicode-range` values, hex colors in any case | preserved |
| Duplicate declarations | preserved, in order |
| Underscore hacks (`_height`) | ordinary identifier, preserved |
| Star hacks (`*zoom`) and other non-identifier names | E0101; use `opaque { }` |
| Literal `#{` | `\#{` in values, or an `opaque` block |

## 7. Opaque preservation guarantees

For unknown at-rules and `opaque { }` blocks Weave guarantees: no interpolation, no evaluation, no statement recognition, no comment removal and no reformatting of the contents. The only transformations are: line breaks written as LF, and (for `opaque`) trimming of leading and trailing whitespace. The block end is found by CSS-aware balanced scanning, so `}` inside strings, comments, escapes and URL tokens does not end it. Opaque content is not validated; it is the author's responsibility, and it can never be used to bypass diagnostics for constructs Weave supports.

## 8. Known v0.1 limitations

- No property/value validation, prefixing or minification.
- `@scope` cannot be nested in rules; `@font-feature-values` and vendor-prefixed at-rules are preserved verbatim only.
- Color values are 8-bit sRGB hex; other color syntaxes are fragments.
- No selector extend/inheritance.
