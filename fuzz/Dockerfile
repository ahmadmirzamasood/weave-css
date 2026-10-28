# Fuzzing environment: Linux, the pinned nightly toolchain and cargo-fuzz.
# Build: docker build -t weave-fuzz fuzz
FROM rust:1.99.1-bookworm
RUN rustup toolchain install nightly-2026-10-08 --profile minimal \
 && rustup default nightly-2026-10-08 \
 && cargo install cargo-fuzz --version 0.13.2 --locked \
 && rm -rf /usr/local/cargo/registry
