# Security policy and engineering requirements

## Status and reporting

No supported release exists yet. Once releases begin, security fixes target the latest minor release until a broader policy is published. Enable GitHub private vulnerability reporting during repository setup. Report vulnerabilities through the repository Security tab -> Report a vulnerability; do not post exploitable details in public issues. If private reporting is unavailable, contact a listed maintainer privately to arrange a channel; no response-time guarantee is currently made.

## Threat model

Untrusted stylesheet/module input can attack the compiler, filesystem resolver, diagnostics, generated output and CI. Compiler memory safety is separate from browser CSS safety: valid emitted CSS can reference remote resources. Weave is not a sanitizer and must not fetch those URLs during compilation.

## Requirements

- No network imports, shell execution, arbitrary plugins or ambient environment access.
- Reject module traversal outside the configured root, including symlink escapes; test canonical identity and cycle detection. Document race limitations if using ordinary filesystem paths; do not claim hardened multi-tenant isolation without stronger OS controls.
- Bound input bytes, files, nesting, call depth, evaluation steps, selector expansion, diagnostic count and emitted bytes. Suggested initial defaults: 16 MiB/file, 128 MiB total input, 4096 modules, parse depth 128, call depth 128, 1 million evaluation steps, 100,000 expanded selectors, 64 MiB output, 100 diagnostics. Validate and tune these through tests; use checked counters. Expose overrides without disabling all limits implicitly.
- Check resource limits before allocations/expansion where possible. Graceful limit diagnostics, not stack overflow or panic.
- CLI writes through temporary sibling files and replaces outputs only on success. Prevent input/output collisions. Do not expose absolute personal paths or source text in published source maps by default.
- No credentials in logs, fixtures, history or artifacts. Escape terminal control characters in diagnostics.
- Audit dependencies, licenses, build scripts and native components. Keep a lockfile. CI handling untrusted PR code receives no secrets or write token; do not use pull_request_target to run that code.
- Pin CI actions, verify release checksums, document provenance. Security exceptions need owner, reason, scope and expiry.

## Verification

Phase 10 must cover traversal, symlinks, recursion/expansion bombs, malformed Unicode input boundaries, terminal escape injection, output collisions, dependency review, and fuzz regressions. Record test evidence and unresolved findings in docs/reports/security-review.md. Do not equate safe Rust with complete security.
