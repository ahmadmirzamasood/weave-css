# Weave function fixtures

Fixtures for phase 05: typed functions with defaults, named arguments, `yield` and recursion; `when`/`otherwise` and `each` with short-circuiting and iteration order; recipes with content blocks and `slot`; `fail`/`warn`; the built-ins of [docs/BUILTINS.md](../../../docs/BUILTINS.md); and the call-depth limit. They are executed by [crates/weave-compiler/tests/fixtures.rs](../../../crates/weave-compiler/tests/fixtures.rs) (`cargo test`), and every case must pass.

The format and rules are those of the [CSS-subset fixtures](../css/README.md): exact bytes for successful builds (compiled twice, validated by the independent CSS parser unless `validation = "preserve-only"`), and exactly the listed diagnostics for error cases. Expected outputs and positions were derived by hand from [LANGUAGE.md](../../../docs/LANGUAGE.md), [TYPES.md](../../../docs/TYPES.md) and [BUILTINS.md](../../../docs/BUILTINS.md) before the runner was executed; one expectation (021, the last binding) was corrected afterwards because both operands of `+` are evaluated before the operator, so the original line failed inside the function first.

Errors found while parsing stop the build before evaluation, so parse-time cases (024, 025) and evaluation-time cases are kept apart. Limits that need non-default values (steps, output, collection sizes, deep recursion on a small stack) are tested in [crates/weave-compiler/tests/functions.rs](../../../crates/weave-compiler/tests/functions.rs).

## Index

| Case | Expect | Covers |
|---|---|---|
| [001-functions](001-functions/) | css | Defaults using module bindings and earlier parameters, positional/named/reordered arguments, early `yield` from conditions and loops, recursion, mutual recursion across hoisted definitions, local shadowing, lexical scope (a function sees its module, never its caller). |
| [002-control-flow](002-control-flow/) | css | `when`/`otherwise when`/`otherwise` with short-circuiting that skips invalid operands; map iteration in insertion order; indexed list iteration; empty loops; nested loops over ranges with positive and negative steps; control flow inside a rule body. |
| [003-recipes](003-recipes/) | css | Recipe defaults in the recipe scope, optional argument lists, hygiene (body sees its module, content block sees its caller), `slot;` placement and order, content passed through nested applications, `slot;` without content, rules emitted at stylesheet level, declaration-only expansion in `@font-face`, conditional slots. |
| [004-builtins](004-builtins/) | css | Every implemented `math`, `list`, `map`, `string` and `meta` built-in and `range`, including quantized rounding, unit conversion, insertion order and Unicode scalar indexing. |
| [020-error-arguments](020-error-arguments/) | error | E4001–E4005 for functions, recipes and defaults; recipes and functions in the wrong role (E1004); unknown recipes (E1002). |
| [021-error-returns](021-error-returns/) | error | `yield` type mismatch (E4006), no `yield` (E4007), `fail` (E9001), non-string `fail` (E2011), `warn` (W9001). |
| [022-error-control-flow](022-error-control-flow/) | error | Non-boolean conditions (E2002) at top level, in `otherwise when` and in functions; non-iterable values and one-variable map loops (E2010); an unguarded division by zero. |
| [023-error-slots](023-error-slots/) | error | A content block executed twice (E4011), also from a loop; a content block never placed (E4010); slots without content are fine. |
| [024-error-statement-contexts](024-error-statement-contexts/) | error | CSS-emitting statements in functions (E4014), `slot` outside recipes (E4012) or in declaration lists (E6005), `yield` outside functions (E4013), nested `fn` (E0104), `apply` in `@keyframes` (E6005). |
| [025-error-definitions](025-error-definitions/) | error | Duplicate parameters, rebound parameters, colliding module names and duplicate loop variables (E1001); keyword parameters (E0103); unknown types, missing `in`, bad `apply` targets, stray `otherwise`, missing parameter lists (E0101). |
| [026-error-recursion](026-error-recursion/) | error | Unbounded function and recipe recursion stop at the call-depth limit (E8003), once each. |
| [027-error-placement](027-error-placement/) | error | Recipe output checked where it is expanded: E6001, E6002, E6003, E6005. |
