# Third-party notices

Weave is released under the [MIT license](LICENSE), Copyright (c) 2026 Weave contributors. This file records the third-party material that is part of, or used to build and test, a Weave release. It is the project's own reading of the licenses, not legal advice.

## In the source repository

The repository contains original work only. No third-party source code, test suite, fixture, example, grammar or documentation has been copied into it; the fixtures in `tests/fixtures`, the examples in `examples/` and the editor grammar in `editors/` were written for this project. The language and CSS specifications it implements (W3C CSS Syntax, CSS Nesting, Selectors; ECMA-426) are cited, not reproduced.

## In the compiler, library and command-line binary

The `weave-compiler` and `weave-css` packages have **no dependencies** other than the Rust standard library (`cargo metadata --locked` lists only workspace packages for the build). Their published package contents are the project's own source, `README.md` and the MIT `LICENSE`.

## In prebuilt binaries

A prebuilt `weave` binary statically links the Rust standard library and the crates that the standard library bundles (such as `hashbrown`, `gimli`, `addr2line`, `miniz_oxide`, `object` and `compiler_builtins`, depending on the target). The Rust project distributes these under `MIT OR Apache-2.0`; Weave elects the MIT terms:

> Copyright (c) The Rust Project Contributors
>
> Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the "Software"), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:
>
> The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.
>
> THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.

The authoritative list of the standard library's own copyright holders and licenses is the `COPYRIGHT` file at <https://github.com/rust-lang/rust>. On Windows the MSVC runtime is dynamically linked from the operating system and is not redistributed.

## Development and test tooling (not distributed)

These are used only to test or measure Weave. They are not linked into any released binary and are not part of the packaged crates (`scripts/check_deps.py` fails if a dev-only package becomes a normal or build dependency).

| Package | Version | License | Used for |
|---|---|---|---|
| `cssparser` | 0.38.0 | MPL-2.0 | an independent CSS parser that validates emitted CSS in tests |
| `dtoa-short` | 0.3.5 | MPL-2.0 | dependency of `cssparser` |
| `dtoa`, `itoa`, `smallvec` | 1.0.11, 1.0.18, 1.16.4 | MIT OR Apache-2.0 | dependencies of `cssparser` |
| `libfuzzer-sys` / LLVM libFuzzer | 0.4.13 | MIT OR Apache-2.0 (LLVM: Apache-2.0 with LLVM exceptions) | the separate `fuzz/` workspace only |
| Google Chrome | the CI runner's | proprietary | computed-style comparison in CI (`scripts/browser_check.py`), run as an external program |

The MPL-2.0 packages are unmodified registry crates used as test-time libraries; their source is available from crates.io, and their file-level copyleft does not extend to Weave's own files.
