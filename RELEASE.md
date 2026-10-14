# Release process

## Milestones

- 0.1.0-alpha.1: phases 01–04; explicitly limited language subset.
- 0.1.0-beta.1: phases 01–08; feature-complete beta subject to fixes.
- 0.1.0: phases 01–11 accepted; stable initial documented feature set, not a 1.0 compatibility promise.
- 1.0.0: PRD acceptance satisfied, grammar/API compatibility policy established, release candidates validated by real projects.

## Before publication

Verify repository and package names, MIT notices and contributor provenance. Record actual supported OS/architecture and minimum Rust version. Do not claim untested targets. Version CLI and library consistently initially. Confirm clean git state and exact release commit; update changelog and migration notes. Test examples, all gates, source maps and CLI install/build instructions. Run dependency/security review and cargo package/publish dry runs for eligible packages. Test packaged contents independently of checkout.

Build release artifacts on supported target runners from the tagged revision. Pin toolchain/action versions, include license, checksums and provenance. Confirm binaries report the intended version. Publish source and build instructions for all binaries. Release jobs use narrowly scoped credentials; prefer supported trusted publishing where configured.

## Publication boundary

The user authorized a public GitHub project and code pushes. Registry publication and signing credentials require configured maintainer access and an explicit release instruction. Phase 11 prepares a release candidate; it must not invent credentials, publish packages, or claim an actual release without authorization and evidence.

## Verification and rollback

Install each published artifact in a clean environment, compile a fixture, and verify checksums. Record release URL/tag/commit. Never move a published tag to different content. For a bad release, publish an advisory and fixed patch; yank a bad registry version where justified without assuming it removes downstream copies. Retain the incident timeline and reproducible regression test.
