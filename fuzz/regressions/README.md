# Fuzz regressions

Minimized inputs that once broke a fuzz target, one directory per target
(`lex/`, `parse/`, `compile/`). Each file is replayed by
`crates/weave-compiler/tests/fuzz_replay.rs`. Name files after the defect
they reproduce and mention the fix in the commit that adds them.

The phase 10 campaigns found no failing input, so no regression is stored
yet. The superlinear-time inputs found by probing in phase 10 are generated
by `crates/weave-compiler/tests/security.rs` instead, because they are too
large to store usefully.
