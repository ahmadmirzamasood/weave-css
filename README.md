# Weave

Weave is a stylesheet language that compiles `.weave` files to standard CSS. The compiler and command line are written in Rust, with no JavaScript, C or C++ runtime and no third-party Rust dependencies. Weave is designed independently; it is not compatible with Sass, Less or other preprocessors.

**Status: release candidate 0.1.0-rc.3.** The v0.1 language is fully implemented, including the `color` built-ins. Nothing has been published to a package registry yet. The grammar and the library API are not declared stable before 1.0; read the [compatibility policy](docs/COMPATIBILITY-POLICY.md) and the [release notes](docs/RELEASE-NOTES-0.1.0-rc.3.md) before depending on it. Weave and `.weave` are the project's names; the [installation guide](docs/INSTALL.md#binary-name-collisions) lists existing tools that also use `weave`.

```weave
bind brand = #0b5fff;
bind steps = [4px, 8px, 16px];

fn space(n: number) -> length {
    yield 4px * n;
}

recipe card(pad: length = space(4)) {
    padding: #{pad};
    border-radius: 6px;
}

rule .button {
    apply card();
    background: #{brand};
    rule &:hover { opacity: 0.9; }
}

each i, step in steps {
    rule .gap-#{i} { gap: #{step}; }
}
```

```css
.button {
  padding: 16px;
  border-radius: 6px;
  background: #0b5fff;
}

.button:hover {
  opacity: 0.9;
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
```

## What it does

- **Compile-time language.** Immutable `bind` values (numbers with units, strings, colors, booleans, lists, ordered maps), expressions, typed `fn` functions, `when`/`each`, reusable `recipe` rules with content blocks, modules (`load`/`export`) and a [built-in library](docs/BUILTINS.md).
- **All of CSS.** Declarations, custom properties and unknown at-rules are preserved, `opaque { … }` blocks are emitted verbatim, and bare words in values are always CSS. Nested rules are lowered to flat CSS with native CSS Nesting specificity, checked against a browser.
- **Safe by construction.** Interpolated values are escaped for where they land and cannot change CSS structure. Every resource (input, nesting, steps, output, …) has a finite limit and fails with a diagnostic; the core performs no filesystem, network, environment or clock access.
- **Deterministic output** with stable error codes, human or JSON diagnostics, ECMA-426 source maps, dependency lists and a polling `watch`.

## Install and use

Prebuilt binaries and registry packages do not exist yet. Build from source (Rust 1.85 or newer):

```bash
git clone https://github.com/ahmadmirzamasood/weave-css
cd weave-css
cargo install --path crates/weave-cli --locked
weavec --version
weavec build examples/design-system/main.weave -o site.css --source-map
```

Details, supported platforms and verification steps: [docs/INSTALL.md](docs/INSTALL.md). Start with the [tutorial](docs/TUTORIAL.md).

## Documentation

| | |
|---|---|
| [Installation](docs/INSTALL.md) | Build from source, platforms, verifying a build |
| [Tutorial](docs/TUTORIAL.md) | A guided tour with checked examples |
| [Language reference](docs/LANGUAGE.md) | Normative v0.1 specification; with [grammar](docs/grammar.ebnf), [types](docs/TYPES.md), [built-ins](docs/BUILTINS.md), [CSS compatibility](docs/CSS-COMPATIBILITY.md) |
| [Command line](docs/CLI.md) | `build`, `check`, `watch`, options, diagnostics, source maps, limits |
| [Library API](docs/LIBRARY.md) | Using `weave-compiler` from Rust |
| [Examples](examples/README.md) | Three complete projects with expected output |
| [Editor support](editors/README.md) | Syntax highlighting (not a language server) |
| [Compatibility policy](docs/COMPATIBILITY-POLICY.md) | What may change before 1.0 |
| [Benchmarks](docs/BENCHMARKS.md) | Method and one-machine results |
| [Release notes](docs/RELEASE-NOTES-0.1.0-rc.3.md), [changelog](CHANGELOG.md) | What is in this candidate and its limits |
| [Releasing](docs/RELEASING.md) | Maintainer procedure, verification, rollback |

## Project

- [Contributing](CONTRIBUTING.md): build, test and review expectations.
- [Security policy](SECURITY.md): threat model and how to report vulnerabilities privately.
- [Third-party notices](THIRD-PARTY-NOTICES.md).
- [MIT license](LICENSE).
