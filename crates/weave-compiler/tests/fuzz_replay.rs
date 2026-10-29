//! Replays the fuzz targets' invariants on the stable toolchain (phase 10):
//! every fixture, example and saved fuzz regression goes through the
//! tokenizer and parser checks of `weave_compiler::fuzzing`, and through a
//! deterministic whole build. Inputs that once failed are kept in
//! fuzz/regressions/<target>/ (see fuzz/README.md).

#![cfg(feature = "fuzzing")]
#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use std::fs;
use std::path::{Path, PathBuf};

use weave_compiler::{CompileOptions, MemoryResolver, ModuleId, compile, fuzzing};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<PathBuf> = entries.map(|e| e.unwrap().path()).collect();
    entries.sort();
    for path in entries {
        if path.is_dir() {
            files(&path, out);
        } else {
            out.push(path);
        }
    }
}

fn inputs() -> Vec<PathBuf> {
    let root = repo_root();
    let mut all = Vec::new();
    for dir in ["tests/fixtures", "examples", "fuzz/regressions"] {
        files(&root.join(dir), &mut all);
    }
    all.retain(|path| {
        let in_regressions = path.components().any(|c| c.as_os_str() == "regressions");
        let source = matches!(
            path.extension().and_then(|e| e.to_str()),
            Some("weave" | "css")
        );
        (in_regressions && path.file_name().is_some_and(|n| n != "README.md")) || source
    });
    all
}

#[test]
fn fuzz_invariants_hold_for_every_fixture_and_regression() {
    let inputs = inputs();
    assert!(inputs.len() > 100, "found only {} inputs", inputs.len());
    for path in &inputs {
        let bytes = fs::read(path).unwrap();
        let text = String::from_utf8_lossy(&bytes);
        fuzzing::lex(&text, 128);
        fuzzing::parse(&bytes, 128);

        // A single-module build is deterministic.
        let build = || {
            let mut resolver = MemoryResolver::new();
            let entry = ModuleId::new("main.weave").unwrap();
            resolver.insert(entry.clone(), bytes.clone());
            let result = compile(&entry, &mut resolver, &CompileOptions::default());
            match result.outcome {
                Ok(output) => Ok(output.css),
                Err(diags) => {
                    for diag in &diags {
                        assert!(
                            !diag.message().starts_with("internal compiler error"),
                            "{}: {}",
                            path.display(),
                            diag.message()
                        );
                    }
                    Err(diags
                        .iter()
                        .map(|d| d.render(&result.sources))
                        .collect::<String>())
                }
            }
        };
        assert_eq!(build(), build(), "{}", path.display());
    }
}
