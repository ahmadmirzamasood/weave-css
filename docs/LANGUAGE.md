# Weave language specification v0.1

Status: **normative v0.1 specification (phase 01)**. No compiler implements it yet. Companion documents are normative for their subjects:

- [grammar.ebnf](grammar.ebnf): syntax.
- [TYPES.md](TYPES.md): values, numeric model, units, coercions, serialization, core built-ins.
- [CSS-COMPATIBILITY.md](CSS-COMPATIBILITY.md): CSS contexts, at-rule matrix, nesting lowering, output ordering.
- [decisions/](decisions/): rationale (ADRs 0002–0008).
- [tests/fixtures/spec/](../tests/fixtures/spec/): specification fixtures, pending a compiler runner.

The words MUST, MUST NOT, SHOULD and MAY are normative. Where this document and a companion disagree, the companion owning the subject wins and the conflict is a specification bug.

Weave is designed independently. It is not compatible with Sass, Less, Stylus or PostCSS syntax, and their behavior is never used as an oracle.

## 1. Design principles

1. **Compile-time code is explicit.** Every compile-time construct starts with a statement keyword (`bind`, `rule`, `when`, …) or is an `#{expression}` interpolation. Everything else is CSS and is emitted as written.
2. **CSS values are literal.** A bare identifier in a CSS value is CSS, never a variable. `color: primary;` emits `color: primary;`.
3. **Data and syntax are separate types.** Strings are data and are always escaped for the context they land in. CSS syntax is produced only by CSS fragments (`` `...` ``), whose tokens are validated.
4. **Deterministic.** Identical inputs and options produce byte-identical output on every platform.
5. **Bounded.** All evaluation is subject to [resource limits](#17-resource-limits).
6. **Unknown CSS survives.** Unknown at-rules and `opaque { }` blocks are preserved verbatim.

## 2. Source text

- Input MUST be UTF-8. Invalid UTF-8 is error E0001. A leading U+FEFF byte-order mark is ignored. U+0000 anywhere is error E0006.
- Line breaks are LF, CRLF, CR or FF. Each counts as one line break for positions. In emitted text every line break is written as LF (CSS Syntax §3.3 preprocessing makes this equivalent).
- Diagnostic positions are 1-based line and 1-based column counted in Unicode scalar values. Spans internally are byte offsets into the original buffer.
- Comments are `/* ... */` only. There are no line comments: `//` is ordinary CSS (for example inside `url(//cdn.example/x.png)`). An unterminated comment is E0003.

## 3. File structure, contexts and statement scanning

A `.weave` file is a **module**. Its top level is a *stylesheet context*. Blocks open other contexts; [CSS-COMPATIBILITY.md §2](CSS-COMPATIBILITY.md#2-context-matrix) lists what each context accepts.

| Context | Opened by | Accepts (summary) |
|---|---|---|
| stylesheet | module top level; `@media`/`@supports`/`@container`/`@layer`/`@starting-style`/`@scope` blocks at stylesheet level | `load`, `export`, `fn`, `recipe` (top level only); `bind`, `when`, `each`, `apply`, `rule`, at-rules, `opaque`, comments |
| rule body | `rule`, recipe body, content block, nested group at-rules inside a rule | declarations, `bind`, `when`, `each`, `apply`, `slot` (recipes only), `rule`, nested group at-rules, unknown at-rules, `opaque` |
| declaration list | `@font-face`, `@property`, `@counter-style`, keyframe blocks, page-margin rules, … | declarations, `bind`, `when`, `each`, `apply` (declaration-only expansion) |
| keyframe list | `@keyframes` | keyframe blocks, `bind`, `when`, `each` |
| page body | `@page` | declarations, page-margin at-rules, `bind`, `when`, `each`, `apply` |
| function body | `fn` | `bind`, `when`, `each`, `yield`, `fail`, `warn` |

### 3.1 Item classification (normative)

At the start of each item in a block or at top level, after skipping whitespace, the parser classifies the item by its first tokens. Whitespace and comments between the tokens named below are skipped.

1. `}` ends the block. `;` is an empty item and is ignored.
2. `/*` starts a comment ([§13](#13-comments-and-opaque-css)).
3. `@` followed by an identifier is an at-rule ([§12](#12-css-at-rules)).
4. An identifier that is a **statement keyword** — `load export bind fn recipe rule when each apply slot yield fail warn opaque` — starts that statement, whatever follows it. So `rule :root { }` is a style rule, and nested `rule :hover { }` is the relative selector `& :hover` (write `rule &:hover` for the same element).
5. A **declaration name** followed by `:` is a declaration. A declaration name is one or more adjacent parts with no whitespace between them, each part being a CSS identifier sequence (including `--custom` names and CSS escapes) or an interpolation `#{…}`. If the whole name is a single unescaped Weave keyword (§4.1; only `as`, `in`, `otherwise`, `true`, `false` can reach this step), the item is E0103. A property whose name equals a keyword is written with a CSS escape (`\72 ule: x;`); no such CSS property exists today.
6. In a keyframe-list context, any other token starts a keyframe block (`from`, `to`, a percentage, or an interpolation).
7. Anything else is error E0110 (“qualified rules must use `rule`”) when it looks like a selector followed by `{`, otherwise E0101. Recovery skips to the next `;` at bracket depth 0 or past the next balanced `{…}` block.

Weave never guesses between a selector and a declaration: nested and top-level style rules are always introduced by `rule`.

### 3.2 Declarations

```
name: value;
name: value !important;
```

- The value is the raw source text from after `:` up to the first `;` or `}` at bracket depth 0, scanned as CSS component values: strings, comments, `url(` tokens, escapes and `()`/`[]`/`{}` nesting are respected, so `;` and `}` inside them do not terminate the value. `#{…}` is an interpolation; `\#{` is a literal `#{` ([§7.4](#74-literal-interpolation-markers)).
- A `{` at depth 0 in the value of a non-custom property is error E0111 (usually a forgotten `;` or an unprefixed selector). Custom properties (`--*`) MAY contain balanced `{}` blocks.
- A trailing `!` + optional whitespace + `important` (ASCII case-insensitive) at depth 0 is importance. It is removed from the value and re-emitted as ` !important`.
- Leading and trailing whitespace of the value is trimmed. Everything else — internal whitespace, comments, case, escapes, duplicate declarations, unknown properties — is preserved. Weave does not validate property names or values.
- The declaration name is emitted as written after interpolation. An interpolated name MUST form a valid CSS identifier sequence (E6007).
- The final declaration of a block MAY omit `;`.

### 3.3 Statements

| Statement | Form |
|---|---|
| load | `load "./path.weave" as name;` |
| export | `export bind …;` / `export fn …` / `export recipe …` |
| bind | `bind name = expression;` |
| fn | `fn name(params) -> type { … }` (return type optional) |
| recipe | `recipe name(params) { … }` (parameter list optional) |
| rule | `rule selector-list { … }` |
| when | `when expr { … } otherwise when expr { … } otherwise { … }` |
| each | `each v in expr { … }`, `each i, v in expr { … }` |
| apply | `apply name(args);`, `apply name(args) { content }`, `apply ns.name(args);` (argument list optional) |
| slot | `slot;` |
| yield | `yield expression;` |
| fail / warn | `fail expression;`, `warn expression;` (expression must be a string) |
| opaque | `opaque { balanced CSS }` |

`fn`, `recipe`, `load` and `export` are allowed only at module top level (E0104). `load` statements MUST precede every other statement except comments and an initial `@charset` (E7005).

## 4. Names, bindings and scope

### 4.1 Identifiers and keywords

An identifier starts with an ASCII letter, `_` or a non-ASCII scalar value and continues with ASCII letters, digits, `_`, `-` or non-ASCII scalar values. Hyphens are part of identifiers: `gap-1` is one name, so subtraction between names needs whitespace (`gap - 1`). A leading `-` is never part of an identifier in expressions.

Keywords (cannot be bound, loaded or used as parameter or function/recipe names, E0103): `load as export bind fn recipe rule when otherwise each in apply slot yield fail warn opaque true false`.

### 4.2 Bindings

- `bind name = expr;` evaluates `expr` once, when the statement executes, and binds an immutable value. There is no assignment or mutation.
- A second binding of the same name in the same scope is E1001. Shadowing a name from an enclosing scope is allowed.
- A name used before its `bind` statement has executed is E1003 (bindings are not hoisted).
- An unresolved name is E1002.

### 4.3 Scopes

Lexical scopes, innermost first:

1. Each block (`rule`, `when`/`otherwise` branch, `each` iteration, at-rule block, content block) opens a scope. Each `each` iteration has a fresh scope holding the loop variables.
2. A function or recipe body shares one scope with its parameters (rebinding a parameter name in the body's top block is E1001).
3. The module scope: top-level `bind`, `fn`, `recipe` and `load` names. Bindings, functions, recipes and module namespaces share one namespace per scope.
4. The prelude: built-in namespaces `css`, `math`, `list`, `map`, `string`, `color`, `meta` and the function `range`. User names MAY shadow prelude names in any scope; the built-in is then unreachable in that scope.

Top-level `fn` and `recipe` definitions are hoisted within their module (mutual recursion works). Function and recipe bodies see their *definition* module scope, never the caller's scope. Module-level bindings referenced from a function body are looked up when the call executes; if the binding has not executed yet, the call fails with E1003.

## 5. Expressions

Expressions appear in `bind`, `when`, `each`, `yield`, `fail`, `warn`, argument lists, defaults and inside `#{…}`. Whitespace and comments are insignificant inside expressions except where they separate tokens.

### 5.1 Literals

| Literal | Examples | Notes |
|---|---|---|
| number | `0`, `12`, `1.5`, `.5`, `16px`, `50%`, `1.25rem` | No exponent notation. A unit is `[A-Za-z]+` or `%`, immediately adjacent. `-4px` is unary minus applied to `4px`. `10%3` is a syntax error; write `10 % 3` for remainder. |
| string | `"text"`, `'text'` | CSS escape rules (TYPES §3); may contain interpolation. |
| color | `#rgb`, `#rgba`, `#rrggbb`, `#rrggbbaa` | Hex digits only; named colors are not literals (use a fragment). |
| boolean | `true`, `false` | |
| list | `[]`, `[1px, 2px,]` | Trailing comma allowed. |
| map | `{}`, `{primary: #2563eb, "on-dark": #fff}` | Keys are identifiers or string literals; duplicate key E2008. |
| CSS fragment | `` `1px solid` ``, `` `calc(100% - #{gap})` `` | CSS component values; may contain interpolation (TYPES §7). |

### 5.2 Precedence and associativity

Highest first.

| Level | Operators | Associativity | Operands |
|---|---|---|---|
| 1 | primary: literal, name, `( expr )` | — | — |
| 2 | call `f(…)`, member `a.b`, index `a[i]` | left | — |
| 3 | unary `-`, `!` | right (prefix) | `-` number; `!` boolean |
| 4 | `*`, `/`, `%` | left | numbers (unit rules in TYPES §2) |
| 5 | `+`, `-` | left | numbers; `+` also string + string |
| 6 | `<`, `<=`, `>`, `>=` | non-associative (`a < b < c` is E0112) | numbers with compatible units, or two strings |
| 7 | `==`, `!=` | non-associative | any two values; never an error |
| 8 | `&&` | left | booleans, short-circuit |
| 9 | `\|\|` | left | booleans, short-circuit |

There is no implicit truthiness: `when`, `!`, `&&` and `||` require booleans (E2002/E2001). `a && b` evaluates `b` only when `a` is `true`; `a || b` evaluates `b` only when `a` is `false`. A skipped operand is not type-checked or evaluated, so it cannot raise errors.

Operands are evaluated left to right. Call arguments are evaluated left to right in source order before the call.

### 5.3 Member and index access

- `ns.name` where `ns` is a loaded module namespace or prelude namespace selects an exported member (E7003 if not exported).
- Otherwise `value.key` is map access with an identifier key; `value[expr]` is map access with a string key or list access with an integer index.
- List indices are zero-based, unitless integers (E2009 otherwise). Out of range, including negative, is E2005. A missing map key is E2007. Use `map.get(m, k, default)` for optional lookup.
- Resolution of `a.b.c` is left to right: the first segment is resolved as a name; a namespace member is resolved next; remaining segments are map keys.

### 5.4 Calls

`f(positional…, name: value…)`. Positional arguments MUST precede named ones (E0113). Each parameter is bound once: duplicates E4003, unknown names E4002, too many positional arguments E4004, missing required arguments E4001. Callable names are user functions, namespace members and prelude built-ins. An unknown function is E1002 — calls are never passed through as CSS functions. Produce CSS function text with a fragment: `` `rgb(0 0 0 / #{alpha})` ``.

## 6. Values

Types: `number` (with optional unit), `string`, `color`, `bool`, `list`, `map`, `css` (fragment). There is no null. Full definitions, the numeric model, units, equality, ordering and serialization are in [TYPES.md](TYPES.md).

## 7. Interpolation and escaping

`#{expression}` evaluates an expression and inserts its serialization into the surrounding CSS text. Interpolation is recognized only in these places. Inside comments, `opaque` blocks, unknown at-rules and `@font-feature-values`, `#{` is ordinary text; anywhere else it is E0101. `load` paths and map-literal string keys do not interpolate.

### 7.1 Contexts

| Context | Where | How each type is inserted |
|---|---|---|
| **value** | declaration values outside strings and `url(…)`; known at-rule preludes; CSS fragments; keyframe selectors | number: `12px`; string: quoted CSS string `"…"`; color: `#rrggbb[aa]`; bool: `true`/`false`; css fragment: its tokens verbatim; list/map: E2004 (use `css.space`, `css.comma`, `css.slash`) |
| **string** | inside a quoted CSS string in a value or prelude, and inside Weave string literals | the text of the value (string contents, number/color/bool serialization, fragment text), escaped as CSS string contents; list/map: E2004 |
| **url** | inside an unquoted `url(…)` | the value's text (as in the string context, before string escaping), escaped for an unquoted URL token: U+0000 → U+FFFD; U+0001–U+0020 and U+007F → `\` + lowercase hex + one space; `"`, `'`, `(`, `)`, `\` → `\` + the character; everything else literal; list/map: E2004 |
| **selector** | `rule` preludes outside attribute-value strings | string/number/bool/color text escaped as an identifier (§7.2); css fragment: tokens verbatim after validation as selector tokens; list/map: E2004 |
| **declaration name** | property names | text escaped as an identifier (§7.2); the complete name must be a valid identifier sequence (E6007); list/map/fragment that is not a single identifier: E2003 |

Weave never re-scans interpolated output for further interpolation.

### 7.2 Identifier escaping

Text inserted in identifier positions is escaped with the CSSOM *serialize an identifier* rules, adjusted for position:

- If the interpolation immediately follows a name code point or `-` in the same compound/name, the text is a *continuation*: name code points (`a-z A-Z 0-9 - _` and non-ASCII) are literal; U+0000 becomes U+FFFD; U+0001–U+001F and U+007F become `\` + lowercase hex + space; every other code point becomes `\` + the code point.
- Otherwise the text *starts* an identifier: additionally a leading digit (or `-` followed by a digit) is hex-escaped (`1` → `\31 `), and a lone `-` is written `\-`.

Example: with `n = 0.5`, `rule .w-#{n} { }` produces `.w-0\.5`; with `n = 10`, `rule .#{n} { }` produces `.\31 0`.

### 7.3 Injection policy (normative)

Interpolation MUST NOT be able to change the structure of the surrounding CSS. Specifically, no interpolated value can introduce: a `;`, `{` or `}` outside a string; an unbalanced `(`, `)`, `[` or `]`; a quote that ends the enclosing string; a comment opener; or a line break inside a string. This holds because:

1. Strings are always serialized as quoted strings or escaped text.
2. Numbers, colors and booleans serialize to fixed token shapes.
3. CSS fragments are validated when created (TYPES §7): balanced brackets, no `{`/`}`, no top-level `;`, no bad-string/bad-url tokens, no unterminated comments. `css.fragment(string)` applies the same validation to runtime text (E2006).
4. Collections never serialize implicitly.

A fragment MAY add commas or combinators to a selector or tokens to a value; that is intended composition, not injection. Weave output is CSS, not HTML: embedding it in an HTML `<style>` element requires HTML-context escaping by the caller.

### 7.4 Literal interpolation markers

Wherever interpolation is recognized, `\#{` produces a literal `#{` and does not start an interpolation. Inside CSS strings this matches CSS semantics (`"\#"` is `"#"`). Outside strings it differs from raw CSS, where `\#` would be an escaped identifier character; write `\23 {` in the rare case that meaning is needed. `opaque { }` blocks, comments and unknown at-rules never interpolate, so `#{` is preserved there unchanged.

## 8. Functions

```weave
fn space(factor: number, base: length = 4px) -> length {
    when factor < 0 { fail "factor must be non-negative"; }
    yield base * factor;
}
```

- Parameters: `name`, optional `: type`, optional `= default`. Parameters with defaults MAY be followed by parameters without defaults only if those are passed by name. Default expressions are evaluated at call time, left to right, in a scope containing the definition module scope and the parameters already bound.
- Type annotations (TYPES §8) are checked when arguments are bound (E4005) and when `yield` executes (E4006). Annotations are optional; omitted means `any`.
- `yield expr;` returns immediately. Reaching the end of the body without `yield` is E4007.
- Function bodies cannot emit CSS: declarations, `rule`, at-rules, `apply`, `slot` and `opaque` inside a function are E4014. Functions are pure: they perform no I/O and their result depends only on arguments and module bindings.
- Recursion is allowed and bounded by the call-depth limit (E8003). Functions are not first-class values.

## 9. Recipes and content blocks

```weave
recipe card(pad: length = 16px) {
    padding: #{pad};
    rule &:hover { box-shadow: `0 1px 2px #0003`; }
    slot;
}
rule .tile {
    apply card(pad: 8px) { border: 0; }
}
```

- A recipe body is a rule-body context. `apply` expands it at the application point: declarations go into the current rule segment, nested rules use the enclosing rule as their parent, and the expansion's output order is the body's statement order.
- The recipe body sees its definition module scope and its parameters, never the caller's scope (hygiene). A content block is evaluated in the **caller's** lexical scope and its output is inserted where `slot;` executes.
- `slot;` outside a recipe body is E4012. If a content block is supplied, the expansion MUST execute `slot` exactly once: zero times is E4010, more than once is E4011. If no content block is supplied, `slot;` emits nothing.
- A `slot;` written inside a content block refers to the slot of the recipe that lexically encloses that content block.
- Applying a recipe whose expansion produces declarations at stylesheet level is E6001; producing rules inside a declaration-list context is E6002.
- Recipe arguments follow the call rules of §5.4.

## 10. Conditions and iteration

```weave
when theme == "dark" { … } otherwise when theme == "dim" { … } otherwise { … }
each size in [4px, 8px] { … }
each i, size in [4px, 8px] { … }       /* i = 0, 1 */
each key, value in {sm: 640px, md: 768px} { … }
each i in range(0, 3) { … }            /* 0, 1, 2 */
```

- Conditions MUST be booleans (E2002). Only the selected branch is evaluated.
- `each v in list` and `each i, v in list` iterate in list order; `i` is a zero-based unitless integer. `each k, v in map` iterates in insertion order with `k` a string. `each v in map` is E2010; iterating any other type is E2010.
- `range(start, end)` and `range(start, end, step)` return a list of unitless integers, half-open, `step` defaults to 1 and MUST be non-zero; `range(3, 0, -1)` is `[3, 2, 1]`. Non-integer arguments E2009. Lists are bounded by the collection limit (E8007).
- `each` iterates over a value computed once before the first iteration.

## 11. Style rules, selectors and nesting

```weave
rule .card, .panel {
    color: red;
    rule &:hover { color: blue; }      /* explicit parent */
    rule .title { margin: 0; }         /* implicit descendant: & .title */
    rule > .icon { width: 1em; }       /* relative: & > .icon */
    margin: 0;                         /* emitted in a later segment */
}
```

- The selector list runs from after `rule` to the first `{` at bracket depth 0 outside strings, comments and interpolations. It is parsed structurally (Selectors Level 4 component values); interpolation is in the selector context.
- `&` is the parent reference. A nested selector that contains no `&` (anywhere, including pseudo-class arguments) is relative: if it starts with a combinator, `&` is inserted before it; otherwise `& ` (descendant) is prepended. `&` at stylesheet level, with no enclosing rule, is E6003.
- Lowering preserves the specificity and matching defined by CSS Nesting Module Level 1 (W3C Working Draft, 22 January 2026), where `&` behaves like `:is(<parent selector list>)`. The exact lowering algorithm, specificity examples and pseudo-element restriction are in [CSS-COMPATIBILITY.md §4](CSS-COMPATIBILITY.md#4-nesting-lowering-and-specificity).
- **Interleaving:** declarations before, between and after nested rules keep their order. Each maximal run of declarations (together with any `opaque` blocks, unknown at-rules and `/*!` comments among them) becomes its own rule segment in output order. A segment with no such items is not emitted.

## 12. CSS at-rules

At-rules keep native CSS syntax and are never prefixed by a Weave keyword. Known at-rules have classified preludes and bodies; preludes are emitted with whitespace runs collapsed to one space and comments removed, and accept interpolation in the value context. Unknown at-rules (including vendor-prefixed names) are preserved verbatim, with no interpolation and no Weave statements inside. Group rules (`@media`, `@supports`, `@container`, `@layer` block, `@starting-style`) nested in rules are lowered by wrapping the current selector. The complete matrix is [CSS-COMPATIBILITY.md §3](CSS-COMPATIBILITY.md#3-at-rule-matrix).

## 13. Comments and opaque CSS

- Comments between items are discarded, except comments starting with `/*!`, which are emitted verbatim at their position (license banners). Comments inside declaration values are part of the value and preserved. Comments inside selector and known at-rule preludes are removed.
- `opaque { … }` contains balanced CSS. The scanner respects strings, comments, escapes and `url(` tokens when finding the matching `}`. The contents, with leading and trailing whitespace trimmed and line breaks written as LF, are emitted verbatim at the block's position: at stylesheet level as top-level CSS; inside a rule body inside the current rule segment. No interpolation or evaluation occurs. `opaque` is not allowed in function bodies (E4014) or declaration-list contexts (E6005).
- Verbatim text is never re-indented: the emitter writes the current indentation before the first line only; later lines keep their source bytes (including source indentation).
- An unknown at-rule is emitted as its exact source text from `@` through its terminating `;` or matching `}`, with line breaks written as LF, under the same indentation rule.

## 14. Modules

```weave
load "./tokens.weave" as tokens;
load "../base/reset.weave" as reset;
```

- Paths MUST start with `./` or `../`, use `/` separators and end in `.weave` (E7004). Absolute paths, URLs, bare specifiers and backslashes are E7004.
- Resolution is relative to the importing file. Module identity is the canonical resolved path (after symlink resolution); a resolved path outside the configured root is E7006. The default CLI root is the entry file's directory; library callers inject a resolver.
- A namespace name is bound in module scope (E1001 on collision with any other module-level name). Loading the same module twice in one file under two names is allowed; it is still evaluated once.
- Only `export`ed bindings, functions and recipes are visible through the namespace (E7003). Imported bindings are read-only values. Loaded modules cannot see the importer.
- Each module is evaluated once per build, the first time it is reached. A cycle is E7002, reported at the `load` that closes it, with the full chain.
- **Emission order:** the build traverses `load` statements depth first in source order. A module's CSS is emitted after the CSS of all modules it loads and only the first time the module is reached (post-order, first encounter). The entry module's own CSS is last. Diamond example in [CSS-COMPATIBILITY.md §5](CSS-COMPATIBILITY.md#5-module-emission-and-css-ordering).
- Native `@import`, `@namespace`, `@charset` and `@layer` statements are CSS, not module loading; they are emitted in place. If the combined output would put `@import` or `@namespace` after a rule or block at-rule that CSS forbids before them, the build fails with E7010 instead of reordering.

## 15. Output format

The default (and in v0.1 only) output style:

- UTF-8, no BOM, LF line breaks, a single trailing LF when non-empty; an empty stylesheet produces zero bytes. No `@charset` is added.
- Top-level items are separated by exactly one empty line; items inside blocks are not separated.
- Indentation is two spaces per block level.
- Rule: `selector-list {` newline, declarations, `}`. Selector lists are joined with `, ` on one line. Complex selectors use one space around combinators (`a > b`, `a + b`, `a ~ b`, descendant `a b`). Whitespace inside `[…]` is removed except a single space before an attribute modifier; pseudo-class argument lists use `, `.
- Declaration: `name: value;` (`name:;` when the value is empty), with ` !important` before the `;` when present.
- At-rule with block: `@name prelude {` … `}`; statement at-rule: `@name prelude;`.
- `@media`/`@supports`/`@container`/`@starting-style` blocks nested inside rules are omitted when lowering leaves them empty. Every other at-rule block — top-level group rules, `@layer` blocks wherever they appear, `@keyframes`, declaration-list at-rules — is always emitted, even when empty (an empty layer block still establishes layer order). An empty block is written `@name prelude {` newline `}`.
- Numbers produced by interpolation follow TYPES §5; literal CSS text is not reformatted.

## 16. Diagnostics

Each diagnostic has a stable code, severity, primary span (file, line, column), message, optional secondary labels and notes. Codes are never reused for a different meaning. Errors stop output generation for the build; the compiler SHOULD continue analysis to report further independent errors up to the diagnostic limit. Terminal control characters in messages are escaped.

| Range | Area |
|---|---|
| E00xx | lexical: E0001 invalid UTF-8, E0002 unterminated string, E0003 unterminated comment, E0004 unterminated interpolation, E0005 unbalanced bracket, E0006 U+0000 in source, E0007 unterminated CSS fragment |
| E01xx | syntax: E0101 unexpected token, E0102 expected `;`, E0103 keyword used as a name or declaration name, E0104 statement allowed only at top level, E0110 qualified rule without `rule`, E0111 `{` in a non-custom declaration value, E0112 chained comparison, E0113 positional argument after named argument |
| E1xxx | names: E1001 duplicate name in scope, E1002 unresolved name, E1003 used before initialization, E1004 wrong kind (value vs function vs recipe vs namespace) |
| E2xxx | types and values: E2001 operand type mismatch, E2002 non-boolean condition, E2003 value not insertable in this context, E2004 list or map interpolated directly, E2005 index out of range, E2006 invalid CSS fragment, E2007 missing map key, E2008 duplicate map key, E2009 integer required, E2010 not iterable / wrong loop arity, E2011 string expected |
| E3xxx | numbers and units: E3001 incompatible units, E3002 overflow or non-finite result, E3003 division or remainder by zero, E3004 unsupported unit product or quotient, E3005 unknown conversion unit |
| E4xxx | calls: E4001 missing argument, E4002 unknown named argument, E4003 duplicate argument, E4004 too many positional arguments, E4005 argument type mismatch, E4006 yield type mismatch, E4007 function ended without `yield`, E4010 content block not placed, E4011 slot executed more than once, E4012 `slot` outside a recipe, E4013 `yield` outside a function, E4014 statement not allowed in a function body |
| E6xxx | CSS contexts: E6001 declaration at stylesheet level, E6002 rule in declaration-list context, E6003 `&` without a parent rule, E6004 nesting under a pseudo-element, E6005 item not allowed in this at-rule context, E6006 `@scope` nested in a rule (unsupported in v0.1), E6007 invalid selector or identifier after interpolation, E6008 invalid `@charset` |
| E7xxx | modules: E7001 module not found, E7002 load cycle, E7003 member not exported, E7004 invalid load path, E7005 `load` after other statements, E7006 path outside root, E7010 native `@import`/`@namespace` order violation |
| E8xxx | limits: E8001 input size, E8002 nesting/parse depth, E8003 call depth, E8004 evaluation steps, E8005 selector expansion, E8006 output size, E8007 collection or string size, E8008 module count |
| E9xxx | user: E9001 `fail` |
| Wxxxx | warnings, same ranges: W9001 `warn` |

## 17. Resource limits

Initial defaults (configurable; overrides never disable all limits implicitly): 16 MiB per file, 128 MiB total input, 4096 modules, nesting/parse depth 128 (blocks, expressions and brackets combined), call depth 128 (functions and recipe applications), 1,000,000 evaluation steps, 100,000 emitted selectors, 64 MiB output, 65,536 elements per list or map, 1 MiB per string or fragment, 100 diagnostics. Exceeding a limit is the matching E8xxx error, never a crash.

## 18. Core built-ins

The prelude provides `range` and the namespaces `css`, `math`, `list`, `map`, `string`, `color`, `meta`. The v0.1 core set and its semantics are listed in [TYPES.md §9](TYPES.md#9-core-built-ins). Phase 05 documents them in `docs/BUILTINS.md` and may add functions only through an ADR. Built-ins are deterministic and perform no I/O.

## 19. Not in v0.1

Selector inheritance/extend, placeholder selectors, mutation, `null`, first-class functions, variadic parameters, overloading, local `fn`/`recipe` definitions, re-exports, wildcard or unqualified imports, network or package imports, transcendental math, colors other than 8-bit sRGB hex, `@scope` nested inside rules, plugins, minified output, source-map embedding policy beyond ARCHITECTURE.md. Source maps, when implemented, follow ECMA-426 (1st edition, December 2024), with columns in UTF-16 code units.

## 20. Complete example

```weave
load "./tokens.weave" as tokens;

bind gap = 8px;
bind palette = {primary: #2563eb, surface: #ffffff};
bind steps = [4px, 8px, 16px];

fn space(factor: number) -> length {
    yield gap * factor;
}

recipe panel(pad: length = 16px) {
    padding: #{pad};
    background: #{palette.surface};
}

rule .card {
    apply panel(space(2));
    color: #{palette.primary};
    display: grid;
    rule &:hover { opacity: 0.8; }
    when tokens.compact { padding: #{gap}; }
}

each i, value in steps {
    rule .gap-#{i} { gap: #{value}; }
}

@media (width >= 768px) {
    rule .card { grid-template-columns: 1fr 2fr; }
}
```

With `tokens.weave` containing `export bind compact = false;`, the output is:

```css
.card {
  padding: 16px;
  background: #ffffff;
  color: #2563eb;
  display: grid;
}

.card:hover {
  opacity: 0.8;
}

.gap-0 {
  gap: 4px;
}

.gap-1 {
  gap: 8px;
}

.gap-2 {
  gap: 16px;
}

@media (width >= 768px) {
  .card {
    grid-template-columns: 1fr 2fr;
  }
}
```
