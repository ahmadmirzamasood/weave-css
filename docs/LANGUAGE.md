# Language proposal — phase 01 must freeze v0.1

## Design boundary

Weave uses explicit statements for compile-time operations. CSS values remain literal token sequences except at `#{expression}` boundaries. Earlier conversational examples used implicit value substitution; this proposal supersedes that ambiguity.

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
rule ".card" {
    apply panel(space(2));
    color: #{palette.primary};
    display: grid;
    rule "&:hover" { opacity: 0.8; }
    when tokens.compact { padding: #{gap}; }
}
each index, value in steps {
    rule ".gap-#{index}" { gap: #{value}; }
}
@media (width >= 768px) {
    rule ".card" { grid-template-columns: 1fr 2fr; }
}
```

`tokens.weave` exports `export bind compact = false;`. Indexing is zero-based.

## Proposed core rules

- UTF-8 input; semicolon-terminated statements and declarations; braced blocks.
- Use CSS `/* comments */`; do not introduce `//` ambiguity with URLs.
- `bind` is immutable, block scoped; duplicate bindings in the same scope are errors; nested scope shadowing is allowed.
- `fn` and `recipe` names resolve lexically; functions return one value via `yield` and cannot emit CSS or perform I/O.
- `when expression { ... } otherwise { ... }` requires a boolean; no implicit truthiness.
- `each value in list`, `each index, value in list`, and `each key, value in map`; iteration preserves collection order. `range(start, end)` is half-open and bounded.
- Lists use `[a, b]`. Maps use `{key: value}` with identifier/string keys and duplicate keys rejected. Missing keys and out-of-range indexing are errors. No null value initially.
- Expression operators: unary `-`, `!`; multiplicative `*`, `/`, `%`; additive `+`, `-`; comparisons; equality; short-circuit `&&`, `||`. Define exact associativity and permitted operand types in phase 01.
- Functions have required/default and named arguments, with duplicate/unknown arguments rejected. Specify argument evaluation order left-to-right. No variadics or overloading initially.
- Recipes use `apply name(args);`. Proposed content blocks: `apply wrapper() { ... }` and `slot;` inside a recipe. Caller content retains caller lexical scope and inserts at the slot's CSS location. Require exactly one slot when a block is supplied in v0.1; diagnose misuse.
- No global mutation, dynamic evaluation, network access, ambient environment access, or implicit filesystem reads from functions.

## Units and serialization

Model numeric dimensions; reject incompatible arithmetic. Define absolute-unit conversion, equality, division, precision, rounding, overflow, and negative zero before implementation. Relative lengths cannot be resolved against a browser environment. CSS `calc()`, `var()`, `env()`, and unknown functions remain literal unless interpolation occurs within them. Never evaluate CSS slash syntax as a language division operator outside interpolation.

Language collections do not implicitly become CSS values. Provide explicit `css.space(list)`, `css.comma(list)`, and `css.slash(left, right)` value constructors with defined serialization. Strings serialize quoted by default; selector interpolation uses a context-specific escaped fragment. Do not silently allow interpolation to inject new declarations or rules.

## Selector and at-rule policy

Nested `rule` without `&` is a descendant. Define nesting equivalent to a chosen current CSS Nesting specification revision, including `:is()`-based specificity behavior where needed. Do not use naive Cartesian expansion if it changes specificity. Preserve interleaved declaration order by emitting separate rule segments. Explicitly test `&` in compound selectors and pseudo-class arguments.

At-rules retain native CSS syntax. Classify known body forms; keyframe steps retain native `from`, `to`, and percentage selectors. Unknown prelude/block tokens are preserved without applying Weave statements inside them.

Provide `opaque { ... }` as a balanced-token CSS escape hatch: emit the inner bytes unchanged, do not interpolate or evaluate them. The scanner must handle braces in strings/comments correctly. This also preserves literal CSS containing `#{...}`. Phase 01 must specify context restrictions and escaping for a literal interpolation marker in ordinary values.

## Module contract

`load "./file.weave" as name;` is top-level only; explicit extension required. `export` exposes bindings, functions, or recipes. Imports are read-only. Resolve relative to importer, canonicalize identity, reject cycles with the chain, load each module once per build. Default CLI module root is the entry file's directory; explicit `--root` permits a wider project boundary. Reject escapes outside root after resolving symlinks. Library callers can provide a virtual resolver.

Emit dependency CSS once in depth-first first-encounter order, with source-order dependency traversal; then emit the importing module's own CSS. This is intentionally independent of the position of `load` among declarations. Preserve valid ordering of emitted CSS `@charset`, `@import`, `@namespace`, and layer statements according to their individual grammar; diagnose illegal combinations rather than silently reorder cascade-sensitive content.

## Required phase 01 outputs

Complete EBNF, a precedence table, a type/coercion table, contextual interpolation/escaping rules, numeric representation decision, nesting specificity examples, module emission examples, a CSS at-rule context matrix, error code conventions, and at least 20 input/expected-output or error fixtures. Mark all unresolved semantics explicitly and settle them before parser work. Proposed syntax is not a claim of implementation.
