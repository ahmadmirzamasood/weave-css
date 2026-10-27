# Weave

Weave is an independently designed stylesheet language that compiles `.weave` files to standard CSS. The compiler and CLI are written in Rust, with no JavaScript, C or C++ runtime. Weave is not compatible with Sass, Less or other preprocessors.

Status: the v0.1 language specification is complete; **the compiler is not implemented yet**. The Rust workspace provides the compiler foundation (sources, spans, diagnostics, options, resource limits, module resolution) and a `weave` CLI whose `build` and `check` commands validate the input's source text and then fail with error E5001 (not implemented). No CSS is produced, and no benchmark or correctness result is claimed. Weave and `.weave` are working names.

```bash
cargo run -p weave-cli -- --help
```

## Language

- [Language specification](docs/LANGUAGE.md)
- [Grammar](docs/grammar.ebnf)
- [Types, numbers and built-ins](docs/TYPES.md)
- [CSS compatibility](docs/CSS-COMPATIBILITY.md)
- [Specification fixtures](tests/fixtures/spec/README.md)

## Project

- [Contributing](CONTRIBUTING.md)
- [Security policy](SECURITY.md)
- [Changelog](CHANGELOG.md)
- [MIT license](LICENSE)
