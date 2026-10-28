# Weave

Weave is an independently designed stylesheet language that compiles `.weave` files to standard CSS. The compiler and CLI are written in Rust, with no JavaScript, C or C++ runtime. Weave is not compatible with Sass, Less or other preprocessors.

Status: the v0.1 language specification is complete; **the compiler implements the plain-CSS subset plus values, scopes, interpolation, functions, control flow, recipes, nesting and modules** (pre-alpha). `weave build` compiles `rule` blocks with nested rules and nested group at-rules (lowered to flat CSS with CSS Nesting specificity, verified against a browser), declarations (including custom properties and `!important`), known at-rules with their CSS body grammar, unknown at-rules and `opaque` blocks preserved verbatim, `/*!` comments and literal `\#{` markers, immutable `bind` statements with lexical scopes, expressions over numbers with units, strings, colors, booleans, lists and maps, `#{…}` interpolation in values, strings, URLs, selectors, property names and preludes, typed functions with defaults, named arguments and recursion, `when`/`otherwise` and `each`, recipes with content blocks and `slot`, `fail`/`warn`, modules (`load "./x.weave" as x;`, `export`, namespaces; resolved relative to the importing file and confined to the input file's directory, symbolic links included; each module emitted once, dependencies first), and the built-ins of [docs/BUILTINS.md](docs/BUILTINS.md). The CLI ([docs/CLI.md](docs/CLI.md)) has `build` (stdout or an atomically replaced `--output` file, stdin input with `-`), `check` and a polling `watch` that keeps the last good output after a failed build; a `--root` for module loading; human or JSON diagnostics; dependency lists; resource-limit overrides; and ECMA-426 source maps (relative paths, no embedded sources unless requested). Basic editor highlighting is in [editors/](editors/README.md) (not a language server). The `color` built-ins are reported as error E5001 (not implemented yet) and never silently ignored. No benchmark result is claimed. Weave and `.weave` are working names.

```bash
cargo run -p weave-cli -- --help
cargo run -p weave-cli -- build examples/design-system/main.weave -o site.css --source-map
```

## Language

- [Language specification](docs/LANGUAGE.md)
- [Grammar](docs/grammar.ebnf)
- [Types, numbers and built-ins](docs/TYPES.md)
- [Built-in reference](docs/BUILTINS.md)
- [CSS compatibility](docs/CSS-COMPATIBILITY.md)
- [Specification fixtures](tests/fixtures/spec/README.md)
- [CSS-subset fixtures](tests/fixtures/css/README.md)
- [Value fixtures](tests/fixtures/values/README.md)
- [Function fixtures](tests/fixtures/functions/README.md)

## Tools

- [Command line, source maps and watch](docs/CLI.md)
- [Examples](examples/README.md)
- [Editor support](editors/README.md)

## Project

- [Contributing](CONTRIBUTING.md)
- [Security policy](SECURITY.md)
- [Changelog](CHANGELOG.md)
- [MIT license](LICENSE)
