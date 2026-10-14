# Product requirements

## Product

Weave is an independently designed stylesheet programming language, distributed as a Rust library and native CLI, producing standard CSS. Target users are frontend developers and tool authors who need reusable design systems, deterministic builds, useful diagnostics, and fast incremental compilation.

## Goals

- G1: Every valid CSS construct can be represented, including unfamiliar future syntax through opaque preservation.
- G2: Compile-time variables, modules, functions, maps/lists, conditions, iteration, recipes and content blocks.
- G3: Rust-only compiler and CLI implementation; no required JS, Sass, Less, C, or C++ runtime.
- G4: Deterministic, reproducible CSS and source maps with clear error locations.
- G5: Measured cold-build and incremental performance, bounded resource use.
- G6: Public GitHub source, MIT license, reproducible release process, contribution documentation.

## Non-goals for 1.0

Existing preprocessor compatibility; selector inheritance/extend semantics; arbitrary plugins or scripts; network imports; package registry for Weave modules; browser runtime; automatic prefixing; aggressive minification; LSP; JS bindings. These may be separate future proposals. Reusable rules are covered by recipes and composition. No claim of copying every Sass-specific feature.

## Functional requirements

| ID | Requirement | Acceptance |
|---|---|---|
| F01 | Own `.weave` syntax | Grammar, syntax highlighting, examples |
| F02 | CSS output | Standards-parser validation for known syntax; opaque preservation fixtures |
| F03 | Variables/scopes | Lexical scope, immutable bindings, shadowing and diagnostics |
| F04 | Values | Numbers, units, strings, booleans, colors, ordered maps, lists, CSS sequences |
| F05 | Expressions/functions | Precedence, typed arguments, pure calls, defaults, named arguments, return values |
| F06 | Reusable rules | Recipes, content blocks, parameters, nested expansion |
| F07 | Control flow | Conditions, finite ranges and collection iteration |
| F08 | Modules | Namespaces, explicit exports, load once, cycle diagnostics, dependency graph |
| F09 | CSS semantics | Nesting/parent selectors, at-rules, custom properties, runtime functions |
| F10 | Tooling | Build/check/watch, stdout, diagnostics, source maps, dependencies |
| F11 | Reliability | Limits, fuzzing, filesystem boundary tests, no input-triggered panics |
| F12 | Speed | Reproducible benchmark report; cold/incremental/memory measurements |

## CSS compatibility contract

Support declarations, selector lists, pseudo selectors, escapes, comments, URLs, custom properties, and all at-rule forms. Known examples include media, supports, container, layer, scope, font-face, keyframes, property, import, namespace, charset, and page. Some have declaration bodies, some rule bodies, some mixed bodies, and some special grammars. Do not parse all blocks as identical stylesheet bodies.

Opaque CSS blocks provide a route for unfamiliar syntax and are emitted verbatim. They cannot be used to justify silently miscompiling supported constructs. Browser support depends on emitted CSS; compilation alone does not polyfill CSS features. Preserve native CSS imports separately from compile-time module loading.

## Release acceptance

1.0 requires all F01–F12 evidence, documented grammar and API stability, repeatable benchmark baselines, supported-platform checks, security limits, accurate source maps, and no known critical/high exploitable compiler vulnerabilities. Alpha versions may cover subsets but must list them explicitly. Never label a subset as full CSS or Sass feature parity.
