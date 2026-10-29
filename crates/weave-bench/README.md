# weave-bench

Benchmark workloads and in-process measurements for the Weave compiler. Not
published; used to produce reproducible performance evidence.

```bash
cargo run --release -p weave-bench -- list
cargo run --release -p weave-bench -- generate target/bench-corpus
cargo run --release -p weave-bench -- run --samples 30 --out target/bench.json
```

The workloads are generated deterministically (no randomness, clock or
environment), and each states what a correct build must contain, so a
faster build cannot win by doing less work:

| workload | what it exercises |
|---|---|
| `small-component` | one component and one token module (typical edit loop) |
| `large-plain` | 20,000 plain CSS rules (tokenizer, parser, emitter) |
| `large-mixed` | 5,000 rules interpolating bindings and arithmetic |
| `module-graph-wide` | 212 modules: 200 components sharing utilities and tokens |
| `module-graph-deep` | a chain of 256 modules |
| `selector-expansion` | nested selector lists four levels deep, nested `@media` |
| `function-heavy` | loops, recursive functions, map/list built-ins, recipes |

In-process scenarios: `warm-fresh` (new cache per build; the process is
warm, so this is not a cold measurement), `warm-fresh-map` (with source
maps), `warm-unchanged` (primed parse cache), and `edit-<label>` (one module
changes before every build). Every cached build is compared with an uncached
build of the same sources.

`scripts/bench.py` adds cold processes (`weavec build` per sample), peak
memory, environment details and corpus hashes, and can measure a baseline
checkout interleaved with the current build:

```bash
python scripts/bench.py run --out-dir target/bench --baseline ../weave-baseline --rounds 4
python scripts/bench.py compare target/bench/before.json target/bench/after.json
```
