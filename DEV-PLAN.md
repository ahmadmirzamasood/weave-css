# Development plan

Execute phases in order; implementation and publication status are distinct. No phase is currently complete. A phase is accepted only when its prompt's gates are evidenced. Estimates are deliberately omitted until phase 02 establishes team throughput; language semantics and compatibility work dominate uncertainty.

| Phase | Deliverable | Prompt |
|---|---|---|
| 00 | Public GitHub bootstrap | [phase-00](prompts/phase-00-github.md) |
| 01 | Freeze the language contract | [phase-01](prompts/phase-01-language-spec.md) |
| 02 | Create the Rust workspace and CI | [phase-02](prompts/phase-02-rust-foundation.md) |
| 03 | Implement tokenizer, parser and plain CSS emission | [phase-03](prompts/phase-03-css-foundation.md) |
| 04 | Implement values, scopes and interpolation | [phase-04](prompts/phase-04-values.md) |
| 05 | Implement functions, control flow and reusable recipes | [phase-05](prompts/phase-05-functions-recipes.md) |
| 06 | Implement nesting and full CSS rule contexts | [phase-06](prompts/phase-06-selectors-rules.md) |
| 07 | Implement module resolution and dependency graph | [phase-07](prompts/phase-07-modules.md) |
| 08 | Finish CLI, source maps and editor basics | [phase-08](prompts/phase-08-developer-tools.md) |
| 09 | Measure and optimize compiler performance | [phase-09](prompts/phase-09-performance.md) |
| 10 | Harden and audit the release candidate | [phase-10](prompts/phase-10-hardening.md) |
| 11 | Prepare the open-source release candidate | [phase-11](prompts/phase-11-release.md) |

## Progress

- [x] 00: Public GitHub bootstrap
- [ ] 01: Freeze the language contract
- [ ] 02: Create the Rust workspace and CI
- [ ] 03: Implement tokenizer, parser and plain CSS emission
- [ ] 04: Implement values, scopes and interpolation
- [ ] 05: Implement functions, control flow and reusable recipes
- [ ] 06: Implement nesting and full CSS rule contexts
- [ ] 07: Implement module resolution and dependency graph
- [ ] 08: Finish CLI, source maps and editor basics
- [ ] 09: Measure and optimize compiler performance
- [ ] 10: Harden and audit the release candidate
- [ ] 11: Prepare the open-source release candidate

## Milestone checkpoints

After 04: alpha subset (CSS plus values). After 08: feature-complete beta candidate. After 10: qualified candidate. After 11: release materials ready; publication requires its own explicit release instruction.

Dependencies: 00 establishes collaboration, 01 defines semantics, 02 provides the foundation, 03–07 implement the compiler, 08 completes user tooling, 09 measures/optimizes, 10 qualifies, 11 packages. Security limits and meaningful tests are added throughout, not deferred until phase 10. Use a focused PR per phase or smaller coherent slices if a phase becomes too large.
