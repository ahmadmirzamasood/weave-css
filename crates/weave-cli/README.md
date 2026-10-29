# weave-css

Installs the `weave` command, the command-line compiler for the [Weave](https://github.com/ahmadmirzamasood/weave-css) stylesheet language. It compiles `.weave` files to standard CSS, with source maps, JSON diagnostics, dependency lists and a polling `watch` mode. Pure Rust, no runtime dependencies.

This is a **release candidate (0.1.0-rc.1)**. The minimum supported Rust version is 1.85.

```bash
cargo install weave-css --version 0.1.0-rc.1 --locked
weave --version
weave build styles/main.weave -o site.css --source-map
```

Other tools install a binary called `weave` too (for example the `weave-cli` crate); see the [installation guide](https://github.com/ahmadmirzamasood/weave-css/blob/main/docs/INSTALL.md#binary-name-collisions).

Documentation: [command-line reference](https://github.com/ahmadmirzamasood/weave-css/blob/main/docs/CLI.md), [tutorial](https://github.com/ahmadmirzamasood/weave-css/blob/main/docs/TUTORIAL.md), [language reference](https://github.com/ahmadmirzamasood/weave-css/blob/main/docs/LANGUAGE.md). Licensed under the MIT license.
