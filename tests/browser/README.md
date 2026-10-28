# Browser computed-style checks

Output snapshots cannot show that lowered CSS behaves like CSS Nesting: matching and specificity decide that. [scripts/browser_check.py](../../scripts/browser_check.py) compiles each case with Weave, derives the native nested stylesheet from the same source by removing the `rule` keyword (the cases use only `rule`, declarations and CSS at-rules, so this is exact), loads both with the case's markup in a headless Chromium-based browser, and requires `getComputedStyle` to agree for every listed property of every element with an `id`.

Each case also has one or more `naive*.css` files: a plausible wrong lowering (Cartesian expansion of a mixed-specificity list, hoisting declarations above nested rules, splicing a complex parent, `:is()` for nested declarations, merged `@media` preludes, …). The script requires each to differ from native nesting on at least one value, so a case that could not detect its error fails.

Run it with a browser that supports native nesting (`CSS.supports("selector(&)")`):

```bash
python3 scripts/browser_check.py
```

The browser is found through `$WEAVE_BROWSER` or common install locations. CI runs the check on Linux with the preinstalled Google Chrome.

| Case | Checks |
|---|---|
| [001-mixed-specificity-list](cases/001-mixed-specificity-list/) | `#main, .side` parent: (1,1,0) for every match, as native `&` |
| [002-interleaved-declarations](cases/002-interleaved-declarations/) | declarations before, between and after nested rules keep their cascade position |
| [003-complex-parent-position](cases/003-complex-parent-position/) | `.x &` with parent `.a .b` matches an `.x` between `.a` and `.b` |
| [004-pseudo-class-arguments](cases/004-pseudo-class-arguments/) | `&` in `:not()`, `:where()`, `:has()`, `:is()`; type parents after other simple selectors |
| [005-nested-group-at-rules](cases/005-nested-group-at-rules/) | `@media`, `@supports`, `@container`, `@layer` nested in rules; nested `@media` not merged |
| [006-nested-declarations-specificity](cases/006-nested-declarations-specificity/) | declarations in nested group rules use per-selector specificity, not `:is()` |
| [007-relative-and-combinators](cases/007-relative-and-combinators/) | leading combinators, `& + &`, three levels with lists |
| [008-spec-fixture-021](cases/008-spec-fixture-021/) | the source of specification fixture 021 |
