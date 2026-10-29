# weave-css

Installs the `weavec` command, the command-line compiler for the [Weave](https://github.com/ahmadmirzamasood/weave-css) stylesheet language. It compiles `.weave` files to standard CSS, with source maps, JSON diagnostics, dependency lists and a polling `watch` mode. Pure Rust, no runtime dependencies.

This is a **release candidate (0.1.0-rc.3)**. The minimum supported Rust version is 1.85.

```bash
cargo install weave-css --version 0.1.0-rc.3 --locked
weavec --version
weavec build styles/main.weave -o site.css --source-map
```

The command is `weavec` because other tools already install a binary called `weave` (see the [installation guide](https://github.com/ahmadmirzamasood/weave-css/blob/main/docs/INSTALL.md#the-binary-name)).

Documentation: [command-line reference](https://github.com/ahmadmirzamasood/weave-css/blob/main/docs/CLI.md), [tutorial](https://github.com/ahmadmirzamasood/weave-css/blob/main/docs/TUTORIAL.md), [language reference](https://github.com/ahmadmirzamasood/weave-css/blob/main/docs/LANGUAGE.md). Licensed under the MIT license.
