# Benchmarks

Weave is designed to be fast, but **no speed claim is made relative to any other tool**: only Weave was measured, on one machine. This page states what was measured, how, and what the numbers do and do not show. Reproduce them with `python scripts/bench.py` (see [Reproducing](#reproducing)).

## Machine and method

| | |
|---|---|
| CPU / RAM | Intel Core i5-9300H (4 cores, 8 threads, 2.4 GHz base), 23.9 GiB |
| OS | Windows 11 Home 10.0.26200, Balanced power plan, on AC power |
| Toolchain | rustc 1.99.1 (`x86_64-pc-windows-gnullvm`), release profile `codegen-units = 1`, `lto = "thin"`, no `RUSTFLAGS` |
| Corpus | 7 deterministic generated workloads (`weave-bench generate`, generator v1), listed below |
| Statistics | medians and interquartile ranges; a difference is called faster or slower only when the IQRs do not overlap; differences below about 5 % are not claimed |

**Cold** means a new `weave build <entry> -o out.css` process per sample with the operating system's file cache warm (one untimed build first). **warm-fresh** is an in-process build with an empty module cache; **warm-unchanged** reuses parsed modules; **edit-** rebuilds after changing one module. Every sample's output is checked: all workloads produced byte-identical CSS in every build compared.

| workload | modules | input bytes | output bytes |
|---|---:|---:|---:|
| small-component | 2 | 708 | 348 |
| large-plain | 1 | 2,652,108 | 2,410,595 |
| large-mixed | 1 | 599,410 | 425,672 |
| module-graph-wide | 212 | 179,797 | 105,619 |
| module-graph-deep | 256 | 25,216 | 6,202 |
| selector-expansion | 1 | 87,895 | 170,632 |
| function-heavy | 1 | 623 | 98,323 |

## Results for this release candidate

An interleaved comparison (4 rounds, the order alternating each round) of the release-candidate tree against the previous `main` (commit `18918cd`, after the hardening phase), 2026-10-29. Medians in milliseconds.

| series | `main` 18918cd | this candidate | verdict |
|---|---:|---:|---|
| cold, `weave --version` (process overhead) | 10.18 | 10.28 | noise |
| cold, small-component | 14.04 | 13.74 | noise |
| cold, large-plain (2.6 MB) | 259.6 | 245.1 | noise |
| cold, large-mixed | 83.7 | 89.5 | noise |
| cold, module-graph-wide (212 modules) | 96.6 | 96.3 | noise |
| cold, module-graph-deep (256 modules) | 72.4 | 69.2 | noise |
| cold, selector-expansion | 25.3 | 27.5 | slower, +8.8 % |
| cold, function-heavy | 47.8 | 47.8 | noise |
| in-process warm-fresh, large-plain | 202.8 | 207.7 | noise |
| in-process edit one leaf of 212 modules | 12.6 | 13.0 | noise |

Peak memory was unchanged (within 1.3 %) on every workload; the largest, large-plain, peaks at 64.4 MiB. In short, the release candidate performs the same as `main` on this machine except for one 8.8 % cold slowdown on selector-expansion that stays inside the 10 % investigation budget set in the project's performance plan. It was not investigated further.

## What the earlier optimization work achieved

Before the candidate, the performance phase reduced cold build times by comparing a baseline and an optimized build in interleaved rounds on the same machine: large-plain 352.5 -> 235.5 ms (-33 %), module-graph-wide 150.5 -> 86.2 ms (-43 %), module-graph-deep 89.9 -> 56.8 ms (-37 %), function-heavy -15 %, selector-expansion -9 %, small files unchanged. Rebuilding after editing one leaf module of the 212-module graph took 12.2 ms in-process, 14 % of a cold build; that figure reuses parsed modules only (evaluation always runs in full).

## Limits of this evidence

- **One machine.** One laptop CPU on Windows. There are no Linux or macOS measurements, so no cross-platform claim is made.
- **A first, non-interleaved run was discarded.** A single run of the candidate compared against the old results from another day showed 10-35 % "slower" on everything, including `weave --version`, which runs none of the compiler. That is machine drift between days, which is why only interleaved comparisons are used above.
- **Cold numbers include the cost of starting the process from Python**; the `weave --version` row measures it (about 10 ms).
- **Nothing here compares Weave with other tools**, and small inputs are dominated by process start.
- **The `watch` rebuild latency and allocation counts are not measured.**
- Raw samples are not published in the repository; rerun the script to produce your own.

## Reproducing

```bash
python scripts/bench.py run --out-dir bench-out                       # measure this checkout
python scripts/bench.py run --out-dir bench-out --baseline ../other-checkout   # interleaved against another checkout
python scripts/bench.py summary bench-out/after.json
python scripts/bench.py compare bench-out/before.json bench-out/after.json
```

Close other programs and use the same power settings. Compare only runs made in the same session.
