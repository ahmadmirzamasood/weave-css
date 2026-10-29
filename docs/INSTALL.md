# Installing Weave

Weave 0.1.0-rc.1 is a release candidate. **No package has been published to a registry and no binary release exists yet**; the supported way to install it today is to build it from source. Sections marked *after publication* describe what will work once a release is published and are not available now.

## Requirements

- A Rust toolchain, version **1.85 or newer** (the minimum supported Rust version). The repository pins **1.99.1** in `rust-toolchain.toml`; with [rustup](https://rustup.rs) the pinned toolchain installs automatically the first time you run `cargo` in the checkout.
- The linker Rust needs on your platform: `cc` on Linux and macOS (Xcode Command Line Tools), the Visual Studio C++ build tools on Windows with the MSVC toolchain. The Weave sources contain no C or C++ and the build downloads no crates (the workspace has no third-party runtime dependencies).
- git, only to clone.

## Build from source

```bash
git clone https://github.com/ahmadmirzamasood/weave-css
cd weave-css
cargo install --path crates/weave-cli --locked
```

This builds in release mode and installs the `weave` binary into `~/.cargo/bin` (`%USERPROFILE%\.cargo\bin` on Windows). To build without installing:

```bash
cargo build --release --locked -p weave-css
./target/release/weave --version
```

Always pass `--locked`; the repository's `Cargo.lock` is part of the reproducible build. To build exactly a release candidate, check out its commit (recorded in the [release notes](RELEASE-NOTES-0.1.0-rc.1.md)) first.

## Verify the installation

```bash
weave --version
```

prints `weave 0.1.0-rc.1` on the first line, followed by the language specification version (`v0.1`) and the tool features. Then compile a file:

```bash
printf 'bind c = #0b5fff;\nrule .a { color: #{c}; }\n' | weave build -
```

```text
.a {
  color: #0b5fff;
}
```

To run the full test suite from the checkout (this is what CI runs):

```bash
cargo test --workspace --all-features --locked
```

## Supported platforms

"Supported" means the build and the test suite run in the project's continuous integration on that platform. Nothing else is claimed.

| Platform | Toolchain | Evidence |
|---|---|---|
| Linux x86_64 (glibc), GitHub `ubuntu-latest` | Rust 1.99.1 and MSRV 1.85 | CI: format, Clippy, full tests; also the clean-container build on Debian 12 |
| macOS (GitHub `macos-latest`, Apple silicon) | Rust 1.99.1 | CI: format, Clippy, full tests |
| Windows x86_64 (GitHub `windows-latest`, MSVC) | Rust 1.99.1 | CI: format, Clippy, full tests |

The prebuilt Linux binary is built on the runner image (Ubuntu with glibc 2.43 at the time of the rehearsal) and needs a glibc at least that new; on an older distribution build from source. Other targets may well work, since the code is portable safe Rust, but they are untested. Intel macOS, Linux musl/ARM and Windows ARM are not tested and no binaries are built for them. The same is true of 32-bit targets: spans use 32-bit offsets and the limits are validated for 64-bit hosts.

## Prebuilt binaries (after publication)

A release will attach one archive per tested platform, a `SHA256SUMS` file and a build-provenance attestation. When that exists, verify before use:

```bash
sha256sum --check --ignore-missing SHA256SUMS         # macOS: shasum -a 256 --check
gh attestation verify weave-0.1.0-rc.1-x86_64-unknown-linux-gnu.tar.gz --repo ahmadmirzamasood/weave-css
```

The archives contain the `weave` binary, `LICENSE`, `THIRD-PARTY-NOTICES.md` and `README.md`. The procedure that produces them is in [RELEASING.md](RELEASING.md).

## From crates.io (after publication)

```bash
cargo install weave-css --version 0.1.0-rc.1 --locked
```

The package is called `weave-css` and installs the `weave` binary. The library is the separate package `weave-compiler` ([Library API](LIBRARY.md)). Pre-release versions are installed only when you name the version, as above.

## Binary name collisions

The binary is called `weave`, the same name as other command-line tools, which was checked on 2026-10-29:

- the crates.io package `weave-cli` (an "entity-level semantic merge" tool) installs a binary named `weave`, and a Homebrew formula named `weave` for the same kind of tool exists;
- the PyPI package `weave` and several unrelated projects use the name for libraries, and the crates.io crate `weave` and the npm packages `weave` and `weave-lang` are taken.

Consequences: `cargo install` refuses to overwrite a binary installed by another package and tells you so; if you use another `weave`, install this one into a separate directory (`cargo install --path crates/weave-cli --locked --root ~/weave-css`, then run `~/weave-css/bin/weave`) or rename the installed file. The maintainers have not yet decided whether to rename the binary before 1.0; see the [release notes](RELEASE-NOTES-0.1.0-rc.1.md#decisions-needed-before-publication).

## Uninstall

```bash
cargo uninstall weave-css
```

## Clean-room build

To check that nothing depends on your machine, build in an empty directory with a fresh Cargo home:

```bash
export CARGO_HOME="$(mktemp -d)"; export CARGO_TARGET_DIR="$(mktemp -d)"
git clone https://github.com/ahmadmirzamasood/weave-css && cd weave-css
cargo build --release --locked -p weave-css && "$CARGO_TARGET_DIR/release/weave" --version
```

Reproducibility of release binaries is described in [RELEASING.md](RELEASING.md#reproducibility).
