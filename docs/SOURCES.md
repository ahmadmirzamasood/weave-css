# Primary references

Checked while preparing the plan on 2026-10-15. Recheck evolving APIs and specifications during implementation. These inform infrastructure/CSS compatibility, not a copied preprocessor grammar.

- Claude Code project instructions and imports: https://code.claude.com/docs/en/memory
- GitHub repository creation: https://docs.github.com/en/repositories/creating-and-managing-repositories/quickstart-for-repositories
- GitHub importing local code: https://docs.github.com/en/migrations/importing-source-code/using-the-command-line-to-import-source-code/adding-locally-hosted-code-to-github
- MIT license: https://opensource.org/license/mit
- CSS Syntax: https://www.w3.org/TR/css-syntax-3/

During phase 01, record the exact CSS Nesting and source-map specification versions used, and add the official Rust/Cargo documentation relevant to the selected toolchain. Do not present unverified package/tool versions as pinned dependencies.

## Phase 01 language references (verified 2026-10-15)

- CSS Syntax Module Level 3, W3C Candidate Recommendation Draft, 1 October 2026: https://www.w3.org/TR/2026/CRD-css-syntax-3-20261001/
- CSS Nesting Module Level 1, W3C Working Draft, 22 January 2026: https://www.w3.org/TR/2026/WD-css-nesting-1-20260122/ (relative selectors, `&` specificity equal to `:is()`, `CSSNestedDeclarations` for interleaved declarations, nested group rules).
- ECMA-426 Source map format, 1st edition, December 2024: https://ecma-international.org/publications-and-standards/standards/ecma-426/ (latest published edition at verification time; the TC39 editor's draft at https://tc39.es/ecma426/ was dated 11 September 2026 and is not normative for Weave). Version field `3`; zero-based columns in UTF-16 code units for CSS; only `version`, `mappings` and `sources` are required.

Rust/Cargo documentation for the chosen toolchain is recorded in phase 02, when the toolchain and MSRV are selected.
