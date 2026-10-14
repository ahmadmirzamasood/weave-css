# ADR 0007: Modules, load placement and CSS emission order

Status: accepted (phase 01, 2026-10-15).

## Decision

- `load "./x.weave" as ns;` only, at the top of a module (after an optional entry `@charset`), with explicit relative paths and the `.weave` extension. No bare specifiers, URLs, wildcard or unqualified imports, or re-exports in v0.1.
- Explicit `export` for bindings, functions and recipes; private by default.
- Canonical path identity, evaluate once per build, cycles are errors with the chain, root confinement after symlink resolution.
- Emission: depth-first traversal in source order, post-order, first encounter; entry module last. All top-level CSS of a loaded module is emitted.
- Native `@import`/`@namespace`/`@charset` are never reordered; an invalid final order is E7010 or E6008.

## Rationale

Requiring loads first makes emission order match reading order (the proposal's "independent of load position" property becomes trivially true). Refusing to reorder native at-rules avoids silently changing the cascade.

## Consequences

Projects that want a dependency's definitions without its CSS must split modules. Conditional loading is impossible by design.
