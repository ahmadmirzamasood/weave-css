# Examples

Each directory is a small project with an entry module `main.weave` and the
CSS it compiles to, `expected.css`. The CLI test
`crates/weave-cli/tests/tools.rs` builds every example and compares the
output byte for byte, and checks that `complete` is exactly the example of
[LANGUAGE.md §20](../docs/LANGUAGE.md#20-complete-example).

| Example | Shows |
|---|---|
| [complete](complete/main.weave) | The language reference's complete example: bindings, maps, a function, a recipe, nesting, `when`, `each`, `@media`, a loaded module |
| [design-system](design-system/main.weave) | Tokens and recipes in modules (`load`, `export`, namespaces), a function from another module, iteration over a map, nested `&:focus-visible` |
| [nesting](nesting/main.weave) | Nested rules, relative selectors and nested `@media` lowered to flat CSS, rule segments around nested rules |

Try one:

```bash
cargo run -p weave-css -- build examples/design-system/main.weave
cargo run -p weave-css -- build examples/design-system/main.weave -o site.css --source-map
cargo run -p weave-css -- watch examples/nesting/main.weave -o nesting.css
```
