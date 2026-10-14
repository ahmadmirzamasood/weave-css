# Performance plan

No benchmark results exist yet. Do not promise a speed multiplier against another compiler.

## Workloads

Version generators and parameters for small component files, medium design systems, large generated stylesheets, deep module graphs, diamond graphs, selector expansion, function-heavy evaluation, and pathological bounded input. Use output-size and semantic checks so a faster run cannot win by omitting work.

Measure cold process start plus compile, warm in-process compile, one-leaf incremental edits, shared-dependency edits, peak resident memory, allocation counts where practical, and output bytes. Record CPU, RAM, OS, Rust version, build flags, revision, fixture hash, cache state, run count, and median/p95. Separate filesystem cache from compiler cache.

## Proposed budgets, not achieved claims

On a named reference machine, adopt an initial investigation threshold of >10% median cold-time or >15% peak-memory regression, confirmed by repeated runs outside expected variance. For a graph edit affecting <=10% of modules, target incremental latency <=30% of cold latency; publish exceptions for unavoidable global work. Set absolute budgets only after phase 09 measurements. Include cold-small-file latency so parallel scheduling does not regress normal usage.

## Optimization sequence

Profile first. Reduce unnecessary clones and intermediate strings; share source buffers; resolve names to IDs; reuse buffers; cache parsed modules with correct dependency invalidation; benchmark concurrency on independent work. Preserve byte-for-byte deterministic output. Add a bytecode VM, custom allocator, SIMD or unsafe code only through a separate measured proposal; unsafe is disallowed by the initial project policy.

Publish before/after raw samples and trade-offs in docs/reports/. Compare external tools only on equivalent tasks with versions and feature differences disclosed.
