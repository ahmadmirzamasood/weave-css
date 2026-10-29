# Compatibility policy

Weave is pre-1.0. This policy says what the maintainers intend to keep stable, what may change, and how changes are announced. It is a statement of intent for the 0.1 series, not a track record: there has been no release yet.

## Versions

Weave uses semantic version numbers, with the usual pre-1.0 reading:

| Release | Meaning |
|---|---|
| `0.1.0-rc.N` | Release candidates. Anything in them may change before `0.1.0`; changes are listed in the changelog. |
| `0.1.x` | Bug fixes and documentation. A source file that compiles under `0.1.x` and uses only documented behavior keeps compiling to the same CSS under `0.1.y` (y > x), except where a fix changes behavior that the specification says was wrong (the changelog lists each). New diagnostics may reject programs that were wrongly accepted. |
| `0.N.0` (N > 1) | May contain breaking changes to the language, output, CLI or library. Each is listed under "Breaking changes" in the changelog with a migration note. |
| `1.0.0` | Not before: the grammar and library API are declared stable, the `color` built-ins exist, and the release candidates have been validated on real projects. |

The compiler version (`weavec --version`, `weave_compiler::VERSION`), the CLI package and the library package share one version number. The language specification has its own version (`v0.1`, `weave_compiler::LANGUAGE_VERSION`), which changes only when the language changes.

## What the 0.1 series keeps stable

- **The language.** The syntax and semantics of [LANGUAGE.md](LANGUAGE.md), [TYPES.md](TYPES.md), [BUILTINS.md](BUILTINS.md) and [CSS-COMPATIBILITY.md](CSS-COMPATIBILITY.md). Where the specification and the compiler disagree, the specification wins and the compiler is fixed. Adding or changing a built-in, a keyword or a diagnostic range is a specification change recorded in the changelog.
- **Unimplemented features become implemented, not different.** A construct that reports `E5xxx` ("not implemented by this build") may start compiling in a later 0.1.x. That is not a breaking change. The `color` built-ins are the known case.
- **Output.** Output is deterministic and byte-identical for identical input and options. Within `0.1.x` the output format of [LANGUAGE.md §15](LANGUAGE.md#15-output-format) does not change; bug fixes that change emitted bytes are listed in the changelog.
- **Diagnostic codes.** A code (`E1002`) never changes meaning or is reused. Message wording, labels and notes may be improved in any release; do not match on them. The `--diagnostics json` format is versioned (`"version":1`); fields may be added without changing the version, and existing fields are not removed or retyped within it.
- **Command line.** Commands, options, exit statuses (0, 1, 2) and the semantics documented in [CLI.md](CLI.md). New options may be added. Human-readable text (help, status lines, human diagnostics) is not an interface; use `--diagnostics json` and the exit status in scripts.
- **Source maps.** The ECMA-426 JSON the CLI writes stays valid for that format. Which mapping points where follows the documented provenance policy.
- **Resource limits.** The limit *names* and their meaning are stable. The *default values* may be raised or lowered in a `0.N.0` release (a limit that is too low for real projects is a bug; see the large-input note in the [release notes](RELEASE-NOTES-0.1.0-rc.2.md#known-limitations)).

## What may change in any 0.x release

- The Rust library API (`weave-compiler`). Types marked `#[non_exhaustive]` may gain fields or variants; functions may change signature. Breaking API changes are listed in the changelog. Items outside the crate root and the public modules `diagnostic`, `options`, `resolve`, `source` and `source_map` are private. The `fuzzing` feature and `weave_compiler::fuzzing` are not an API at all.
- The `weave-bench` package and everything under `scripts/` and `fuzz/`.
- Performance characteristics. Speed is measured and reported, never promised.
- Watch-mode implementation details (it polls) and the exact set of platforms with prebuilt binaries.
- The names. "Weave", the `weavec` binary, the `.weave` extension and the package names are working choices. Renaming any of them before 1.0 would be a breaking change and would be announced in the release notes with a migration path. The [release notes](RELEASE-NOTES-0.1.0-rc.2.md#decisions-needed-before-publication) record why this is open.

## Supported toolchain and platforms

- **Minimum supported Rust version: 1.85**, checked in CI. Raising it is a `0.N.0` change (or, in a `0.1.x`, only when required by a security fix) and is listed in the changelog.
- **Platforms:** those in the [installation guide](INSTALL.md#supported-platforms). A platform is dropped only in a `0.N.0` release with a changelog entry.

## Deprecation

A construct is deprecated by a warning diagnostic and a changelog entry in one release and removed no earlier than the next `0.N.0` release. Security fixes may skip this process; they are described in an advisory.

## Reporting a compatibility problem

If a release changed documented behavior without a changelog entry, that is a bug. Open an issue with the input, the old and new output, and the two versions. Security problems follow [SECURITY.md](../SECURITY.md) instead.

## Migration

- **From other CSS preprocessors:** Weave is not compatible with Sass, Less, Stylus or PostCSS syntax and offers no converter. Concepts map across (variables to `bind`, mixins to `recipe`, functions to `fn`, module imports to `load`/`export`), but syntax and semantics differ; use the [tutorial](TUTORIAL.md).
- **From plain CSS:** a `.css` file becomes a `.weave` file by prefixing each style rule's selector with `rule` (declarations and at-rules are unchanged). This is the only systematic change, and it is deliberate: Weave never guesses between a selector and a declaration.
- **Between Weave releases:** `0.1.0-rc.2` is the first release; there is nothing to migrate from.
