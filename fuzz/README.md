# Fuzzing

Coverage-guided fuzz targets for the Weave compiler, built with
[cargo-fuzz](https://github.com/rust-fuzz/cargo-fuzz) and libFuzzer. This
crate is not a member of the main workspace: it needs a nightly toolchain and
libFuzzer (C++, compiled by `libfuzzer-sys`), and neither is required to build
or use Weave.

Fuzzing is evidence, not proof: a campaign that finds nothing shows only that
the inputs it reached did not break the checked invariants.

## Targets

| target | input | checks |
|---|---|---|
| `lex` | bytes, decoded lossily to UTF-8 | in both tokenizer modes: tokens are contiguous, non-empty, on character boundaries, cover the input and end with `Eof` within one token per byte |
| `parse` | raw bytes | source registration (UTF-8, U+0000, byte-order mark) and parsing as an entry module: every diagnostic span lies inside the file on character boundaries and resolves to a location; rendered diagnostics contain no control characters |
| `compile` | option byte, then up to four modules separated by `0xFF` | whole builds with lowered limits (50,000 steps, 1 MiB output, 10,000 selectors, …): no "internal compiler error", two uncached builds and a cached rebuild are identical (CSS, source map, diagnostics), every diagnostic location resolves, rendered diagnostics contain no control characters, default source-map JSON has no `sourcesContent` |

All targets also detect panics, and libFuzzer reports inputs that take longer
than the `-timeout` (10 s) or exceed `-rss_limit_mb` (2048).

The `compile` target names its modules `main.weave` (entry), `a.weave`,
`lib/b.weave` and `lib/c.weave`. Option byte bits: 1 source map, 4 nesting
depth 512, 8 call depth 512, 16 diagnostic limit 3.

## Corpus and dictionary

`seed.py OUT` builds the seed corpora from `tests/fixtures` and `examples`:
every `.weave` and `.css` file seeds `lex` and `parse`; each fixture or
example directory seeds `compile` with its modules renamed to the target's
module names (load paths rewritten) and four option bytes. `weave.dict` holds
Weave keywords, built-in names, CSS at-rules, punctuation and hostile bytes
(BOM, CRLF, U+202E, ESC, 0xFF).

## Running

libFuzzer does not build for the `x86_64-pc-windows-gnullvm` toolchain, so the
campaigns run on Linux. With Docker:

```sh
docker build -t weave-fuzz fuzz
docker run --rm -v "$PWD:/src:ro" -v "$PWD/fuzz-out:/out" weave-fuzz \
  sh /src/fuzz/run.sh compile 600 -fork=2
```

`run.sh TARGET SECONDS [libFuzzer flags]` copies the repository into the
container, seeds the corpus (once), and writes the corpus, artifacts and log
to `/out/TARGET`. On a Linux host with the nightly toolchain from
`Dockerfile` and cargo-fuzz installed, the same commands work directly:

```sh
python3 fuzz/seed.py fuzz/corpus
cd fuzz
cargo +nightly-2026-10-08 fuzz run -O --debug-assertions compile corpus/compile -- \
  -dict=weave.dict -max_total_time=600 -timeout=10 -rss_limit_mb=2048 -max_len=65536
```

CI runs each target for 60 seconds ("Fuzz smoke"), which checks the harness
and the seeds; it is not a campaign.

## Findings

A failing input is minimized (`cargo fuzz tmin TARGET ARTIFACT`), saved in
`regressions/TARGET/` and covered by a regression test before it is fixed.
`crates/weave-compiler/tests/fuzz_replay.rs` replays every saved regression,
fixture and example through the target invariants on the stable toolchain,
so `cargo test --workspace --all-features` keeps them fixed.
