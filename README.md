# Weave

Weave is an independently designed stylesheet language that compiles `.weave` files to standard CSS. The compiler and CLI are written in Rust, with no JavaScript, C or C++ runtime. Weave is not compatible with Sass, Less or other preprocessors.

Status: the v0.1 language specification is complete; **the compiler implements only its plain-CSS subset** (pre-alpha). `weave build` compiles `rule` blocks at stylesheet level, declarations (including custom properties and `!important`), known at-rules with their CSS body grammar, unknown at-rules and `opaque` blocks preserved verbatim, `/*!` comments and literal `\#{` markers, and writes CSS to stdout; `weave check` does the same without output. Bindings, control flow, functions, recipes, modules, `#{…}` interpolation and nesting are reported as error E5001 (not implemented yet) and never silently ignored. No benchmark result is claimed. Weave and `.weave` are working names.

```bash
cargo run -p weave-cli -- --help
```

## Language

- [Language specification](docs/LANGUAGE.md)
- [Grammar](docs/grammar.ebnf)
- [Types, numbers and built-ins](docs/TYPES.md)
- [CSS compatibility](docs/CSS-COMPATIBILITY.md)
- [Specification fixtures](tests/fixtures/spec/README.md)
- [CSS-subset fixtures](tests/fixtures/css/README.md)

## Project

- [Contributing](CONTRIBUTING.md)
- [Security policy](SECURITY.md)
- [Changelog](CHANGELOG.md)
- [MIT license](LICENSE)
