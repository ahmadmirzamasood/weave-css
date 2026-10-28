#!/bin/sh
# Runs one bounded fuzz campaign inside a Linux container with a nightly
# toolchain and cargo-fuzz (see fuzz/README.md).
#
# Usage: fuzz/run.sh TARGET SECONDS [extra libFuzzer flags]
# Expects the repository at /src (read-only is fine) and writes the corpus,
# artifacts and log to /out/TARGET.
set -eu
target=$1
seconds=$2
shift 2
mkdir -p /work /out/"$target"/corpus /out/"$target"/artifacts
# Work on a copy so the host tree is never written by the container.
cp -r /src/crates /src/fuzz /src/tests /src/examples /src/Cargo.toml /work/ 2>/dev/null || true
cd /work/fuzz
if [ ! -d /out/seed ]; then python3 seed.py /out/seed; fi
cp -n /out/seed/"$target"/* /out/"$target"/corpus/ 2>/dev/null || true
cargo fuzz build -O --debug-assertions "$target"
cp Cargo.lock /out/Cargo.lock
cargo fuzz run -O --debug-assertions "$target" /out/"$target"/corpus -- \
  -dict=weave.dict -max_total_time="$seconds" -timeout=10 -rss_limit_mb=2048 \
  -max_len=65536 -print_final_stats=1 -artifact_prefix=/out/"$target"/artifacts/ "$@" \
  2>&1 | tee /out/"$target"/log.txt
