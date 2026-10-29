# Security policy and engineering requirements

## Status and reporting

**Supported versions.** No stable release exists yet; the current release candidate is 0.1.0-rc.1. Until 1.0, security fixes are made against the latest release (or release candidate) only; older pre-releases are not patched, and users should move to the newest one.

**Reporting a vulnerability.** Report privately: open <https://github.com/ahmadmirzamasood/weave-css/security/advisories/new> (repository Security tab -> Report a vulnerability). Do not post exploitable details in public issues, pull requests or discussions. Include the Weave version (`weave --version`), your platform, the smallest input that shows the problem and what you expected. Examples of in-scope problems: a crash, hang or unbounded memory use caused by a stylesheet; reading or writing a file outside the configured root or over an input; output that lets interpolated data change CSS structure; terminal-control injection through diagnostics; supply-chain problems in the repository or release artifacts. Out of scope: CSS that a browser later interprets in a harmful way (Weave is not a sanitizer), and races against concurrent writers to the module tree (documented in docs/CLI.md).

**What to expect.** This is a volunteer project and makes no response-time guarantee. The maintainers aim to acknowledge a report, agree on a fix and disclose in a published advisory with credit (if you want it) after a fixed release exists. Registry versions with a vulnerability are yanked where justified and an advisory is published; see docs/RELEASING.md. If private reporting is unavailable, contact a maintainer through the address on their GitHub profile to arrange a private channel.

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

Before a release, verification must cover traversal, symlinks, recursion/expansion bombs, malformed Unicode input boundaries, terminal escape injection, output collisions, dependency review, and fuzz regressions. Record test evidence and unresolved findings with the release. Do not equate safe Rust with complete security.
