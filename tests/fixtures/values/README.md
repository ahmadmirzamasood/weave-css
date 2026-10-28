# Weave value fixtures

Fixtures for phase 04: immutable bindings and lexical scopes, numbers with units, strings, colors, booleans, lists and maps, expression precedence and short-circuiting, the explicit CSS sequence constructors (`css.space`, `css.comma`, `css.slash`, `css.ident`, `css.fragment`) and `#{…}` interpolation in every CSS context. They are executed by [crates/weave-compiler/tests/fixtures.rs](../../../crates/weave-compiler/tests/fixtures.rs) (`cargo test`), and every case must pass.

The format and rules are those of the [CSS-subset fixtures](../css/README.md): exact bytes for successful builds (compiled twice, validated by the independent CSS parser unless `validation = "preserve-only"`), and exactly the listed diagnostics for error cases. Expected outputs and positions were derived by hand from [LANGUAGE.md](../../../docs/LANGUAGE.md) and [TYPES.md](../../../docs/TYPES.md) before the runner was executed.

Errors found while parsing (expression syntax, fragment literals, duplicate bindings, out-of-range literals) stop the build before evaluation, so parse-time and evaluation-time errors are kept in separate cases.

## Index

| Case | Expect | Covers |
|---|---|---|
| [001-bindings-and-scopes](001-bindings-and-scopes/) | css | Top-level bindings; rule, group at-rule and keyframe-list blocks open scopes whose bindings shadow outer ones only inside them; a user binding may shadow a prelude name. |
| [002-numbers-and-units](002-numbers-and-units/) | css | Numeric boundaries (±1e12), six-decimal quantization, conversions within each unit category, unknown units, negation keeping the unit, string ordering, structural equality of lists and maps. |
| [003-strings-and-escaping](003-strings-and-escaping/) | css | Weave string escapes; value-context quoting; string-context escaping inside double- and single-quoted CSS strings; nested interpolation in strings; text of numbers, colors, booleans and fragments; url-context escaping. |
| [004-maps-and-lists](004-maps-and-lists/) | css | Map access by name and string key, nested list and map access, computed indices, empty-list equality, insertion order through `css.comma`. |
| [005-css-constructors](005-css-constructors/) | css | `css.space`, `css.comma`, `css.slash` with positional and named arguments, `css.ident`, `css.fragment`, fragment literals with interpolation, an identifier fragment in an at-rule prelude. |
| [006-browser-functions](006-browser-functions/) | css | `var()`, `calc()`, `env()`, `max()`, `anchor()`, unknown functions and bare identifiers stay CSS; only explicit interpolations inside them are evaluated. |
| [007-interpolation-contexts](007-interpolation-contexts/) | css | Identifier escaping by position in selectors and property names; hostile text quoted in values, escaped in strings, attribute values and URLs; interpolation in media preludes and keyframe selectors. |
| [020-error-use-before-bind](020-error-use-before-bind/) | error | Bindings are not hoisted: a name used before its block binds it is E1003, even with an outer binding. |
| [021-error-operand-types](021-error-operand-types/) | error | No implicit coercion: wrong operand types for `+`, unary `-`, `!`, `&&` and `<` are E2001 at the operator. |
| [022-error-units-and-overflow](022-error-units-and-overflow/) | error | Unit products and inverse units (E3004), remainder by zero (E3003), overflow beyond 1e12 (E3002), ordering across unit and unitless (E3001). |
| [023-error-indexes-and-keys](023-error-indexes-and-keys/) | error | Negative and non-integer list indices, missing map keys, non-string map keys, indexing and members on the wrong types. |
| [024-error-names-and-calls](024-error-names-and-calls/) | error | Unresolved names and functions, namespaces and values in the wrong role, unknown prelude members, unimplemented built-ins (E5001), built-in argument binding and type errors. |
| [025-error-interpolation-contexts](025-error-interpolation-contexts/) | error | Collections never interpolate; declaration names accept only single-identifier fragments; emptied selector compounds (E6007); `&` from a fragment at stylesheet level (E6003); comment openers formed across an interpolation boundary (E2003). |
| [026-error-expression-syntax](026-error-expression-syntax/) | error | Parse-time expression errors, one per statement with recovery after it: invalid fragment literals, a unit followed by digits, chained comparisons, positional after named arguments, duplicate map keys, keywords as names, duplicate bindings, an out-of-range literal, a missing `;`. |
