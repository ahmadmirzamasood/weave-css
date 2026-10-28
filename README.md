# Weave

Weave is an independently designed stylesheet language that compiles `.weave` files to standard CSS. The compiler and CLI are written in Rust, with no JavaScript, C or C++ runtime. Weave is not compatible with Sass, Less or other preprocessors.

Status: the v0.1 language specification is complete; **the compiler implements the plain-CSS subset plus values, scopes, interpolation, functions, control flow, recipes and nesting** (pre-alpha). `weave build` compiles `rule` blocks with nested rules and nested group at-rules (lowered to flat CSS with CSS Nesting specificity, verified against a browser), declarations (including custom properties and `!important`), known at-rules with their CSS body grammar, unknown at-rules and `opaque` blocks preserved verbatim, `/*!` comments and literal `\#{` markers, immutable `bind` statements with lexical scopes, expressions over numbers with units, strings, colors, booleans, lists and maps, `#{…}` interpolation in values, strings, URLs, selectors, property names and preludes, typed functions with defaults, named arguments and recursion, `when`/`otherwise` and `each`, recipes with content blocks and `slot`, `fail`/`warn`, and the built-ins of [docs/BUILTINS.md](docs/BUILTINS.md), and writes CSS to stdout; `weave check` does the same without output. Modules and the `color` built-ins are reported as error E5001 (not implemented yet) and never silently ignored. No benchmark result is claimed. Weave and `.weave` are working names.

```bash
cargo run -p weave-cli -- --help
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

## Project

- [Contributing](CONTRIBUTING.md)
- [Security policy](SECURITY.md)
- [Changelog](CHANGELOG.md)
- [MIT license](LICENSE)
