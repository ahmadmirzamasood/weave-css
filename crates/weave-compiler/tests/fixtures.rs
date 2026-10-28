//! Executable fixture runner.
//!
//! - `tests/fixtures/css` (phase 03, plain-CSS subset),
//!   `tests/fixtures/values` (phase 04, values, scopes and interpolation)
//!   `tests/fixtures/functions` (phase 05, functions, control flow,
//!   recipes and built-ins) and `tests/fixtures/nesting` (phase 06, nesting
//!   and rule contexts).
//!   Every case must pass: exact expected bytes (and identical output on a
//!   second run) or exactly the expected diagnostics. Successful output is
//!   also parsed by an independent CSS parser unless the case declares
//!   `validation = "preserve-only"` (syntax that parser may not understand).
//! - `tests/fixtures/spec`: the phase 01 specification fixtures (all pass
//!   since phase 07; multi-module cases load sibling `*.weave` files). A case
//!   passes, is pending (the build reports E5001 for a construct it does not
//!   implement yet) or fails. No case may fail, and the set of passing cases
//!   is pinned so that progress and regressions are both visible.
//!
//! Formats: tests/fixtures/spec/README.md.

#![allow(
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::unwrap_used
)]

mod independent;

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use weave_compiler::{
    Code, CompileOptions, Diagnostic, MemoryResolver, ModuleId, SourceManager, compile,
};

/// Specification fixtures that this build compiles as specified.
const SPEC_PASSING: [&str; 35] = [
    "001-plain-css-preservation",
    "002-bindings-value-interpolation",
    "003-lexical-scope-shadowing",
    "004-error-duplicate-binding",
    "005-error-unresolved-name",
    "006-numbers-units-precision",
    "007-error-incompatible-units",
    "008-precedence-short-circuit",
    "009-error-division-by-zero",
    "010-strings-contexts-escaping",
    "011-maps-lists-sequences",
    "012-error-list-interpolation",
    "013-error-index-out-of-range",
    "014-functions-defaults-named-recursion",
    "015-error-unknown-named-argument",
    "016-recipes-content-blocks",
    "017-error-content-block-not-placed",
    "018-conditions-ranges-selector-escaping",
    "019-error-non-boolean-condition",
    "020-nesting-parent-references",
    "021-nesting-specificity-is",
    "022-nested-group-at-rules",
    "023-at-rule-body-contexts",
    "024-opaque-and-unknown-syntax",
    "025-modules-diamond-namespaces",
    "026-error-module-cycle",
    "027-error-member-not-exported",
    "028-error-native-import-order",
    "029-error-selector-without-rule",
    "030-interpolation-injection-safety",
    "031-error-invalid-fragment",
    "032-error-argument-type",
    "033-error-top-level-declaration",
    "034-error-nesting-under-pseudo-element",
    "035-unicode-and-crlf",
];

fn fixtures(dir: &str) -> Vec<PathBuf> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures")
        .join(dir);
    let mut cases: Vec<PathBuf> = fs::read_dir(&root)
        .unwrap_or_else(|e| panic!("{}: {e}", root.display()))
        .map(|entry| entry.expect("directory entry").path())
        .filter(|path| path.join("case.toml").is_file())
        .collect();
    cases.sort();
    cases
}

fn name(case: &Path) -> String {
    case.file_name().unwrap().to_string_lossy().into_owned()
}

/// Reads the `key = "string"` entries of a case.toml (other values, such as
/// arrays, are ignored).
fn case_toml(case: &Path) -> BTreeMap<String, String> {
    let text = fs::read_to_string(case.join("case.toml")).expect("case.toml");
    let mut map = BTreeMap::new();
    for line in text.lines() {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let value = value.trim();
        if let Some(inner) = value.strip_prefix('"').and_then(|v| v.strip_suffix('"')) {
            map.insert(key.trim().to_string(), inner.replace("\\\"", "\""));
        }
    }
    map
}

struct Run {
    sources: SourceManager,
    outcome: Result<String, Vec<Diagnostic>>,
}

/// Compiles a case with every `*.weave` file of its directory available.
fn run(case: &Path) -> Run {
    let mut resolver = MemoryResolver::new();
    for entry in fs::read_dir(case).expect("case directory") {
        let path = entry.expect("entry").path();
        if path.extension().is_some_and(|e| e == "weave") {
            let id = ModuleId::new(name(&path)).expect("module id");
            resolver.insert(id, fs::read(&path).expect("read module"));
        }
    }
    let entry = ModuleId::new("main.weave").expect("entry id");
    let compilation = compile(&entry, &mut resolver, &CompileOptions::default());
    Run {
        sources: compilation.sources,
        outcome: compilation.outcome.map(|output| output.css),
    }
}

#[derive(Debug)]
struct Expected {
    severity: String,
    code: String,
    file: String,
    line: u32,
    column: u32,
    substring: String,
}

fn expected_diagnostics(case: &Path) -> Vec<Expected> {
    let text = fs::read_to_string(case.join("expected.diag")).expect("expected.diag");
    text.lines()
        .filter(|l| !l.trim().is_empty() && !l.starts_with('#'))
        .map(|line| {
            let (head, quoted) = line.split_once(" \"").expect("message substring");
            let mut parts = head.split(' ');
            let severity = parts.next().unwrap().to_string();
            let code = parts.next().unwrap().to_string();
            let location = parts.next().unwrap();
            let mut loc = location.rsplitn(3, ':');
            let column = loc.next().unwrap().parse().unwrap();
            let line_no = loc.next().unwrap().parse().unwrap();
            let file = loc.next().unwrap().to_string();
            Expected {
                severity,
                code,
                file,
                line: line_no,
                column,
                substring: quoted.strip_suffix('"').unwrap().to_lowercase(),
            }
        })
        .collect()
}

fn matches(expected: &Expected, actual: &Diagnostic, sources: &SourceManager) -> bool {
    let Some((file, lc)) = actual.primary().and_then(|span| sources.location(span)) else {
        return false;
    };
    let text = std::iter::once(actual.message())
        .chain(actual.notes().iter().map(String::as_str))
        .collect::<Vec<_>>()
        .join("\n")
        .to_lowercase();
    actual.severity().as_str() == expected.severity
        && actual.code().as_str() == expected.code
        && file == expected.file
        && lc.line == expected.line
        && lc.column == expected.column
        && text.contains(&expected.substring)
}

/// Whether every expected diagnostic is matched by a distinct actual one.
fn all_matched(expected: &[Expected], actual: &[Diagnostic], sources: &SourceManager) -> bool {
    let mut used = vec![false; actual.len()];
    expected.iter().all(|exp| {
        let found = actual
            .iter()
            .enumerate()
            .find(|(i, a)| !used[*i] && matches(exp, a, sources));
        match found {
            Some((i, _)) => {
                used[i] = true;
                true
            }
            None => false,
        }
    })
}

fn render(diags: &[Diagnostic], sources: &SourceManager) -> String {
    diags.iter().map(|d| d.render(sources)).collect()
}

#[test]
fn css_fixtures() {
    exact_fixtures("css", 30);
}

#[test]
fn values_fixtures() {
    exact_fixtures("values", 14);
}

#[test]
fn functions_fixtures() {
    exact_fixtures("functions", 12);
}

#[test]
fn nesting_fixtures() {
    exact_fixtures("nesting", 9);
}

/// Runs a fixture set in which every case must pass exactly.
fn exact_fixtures(dir: &str, minimum: usize) {
    let cases = fixtures(dir);
    assert!(cases.len() >= minimum, "expected the {dir} fixtures");
    let mut failures = Vec::new();
    for case in &cases {
        let meta = case_toml(case);
        let first = run(case);
        let problem = match (meta.get("expect").map(String::as_str), &first.outcome) {
            (Some("css"), Ok(css)) => {
                let expected = fs::read_to_string(case.join("expected.css")).expect("expected.css");
                let second = run(case).outcome.ok();
                if *css != expected {
                    Some(format!(
                        "output differs\n--- expected ---\n{expected}--- actual ---\n{css}"
                    ))
                } else if second.as_deref() != Some(css.as_str()) {
                    Some("output is not deterministic".to_string())
                } else if meta.get("validation").map(String::as_str) == Some("preserve-only") {
                    None
                } else {
                    let (_, problems) = independent::parse(css);
                    (!problems.is_empty())
                        .then(|| format!("independent parser: {}", problems.join("; ")))
                }
            }
            (Some("css"), Err(diags)) => Some(format!(
                "expected CSS, got diagnostics:\n{}",
                render(diags, &first.sources)
            )),
            (Some("error"), Err(diags)) => {
                let expected = expected_diagnostics(case);
                if all_matched(&expected, diags, &first.sources) && expected.len() == diags.len() {
                    None
                } else {
                    Some(format!(
                        "diagnostics differ\n--- expected ---\n{expected:#?}\n--- actual ---\n{}",
                        render(diags, &first.sources)
                    ))
                }
            }
            (Some("error"), Ok(css)) => Some(format!("expected an error, got CSS:\n{css}")),
            (other, _) => Some(format!("invalid expect value {other:?}")),
        };
        if let Some(problem) = problem {
            failures.push(format!("{}: {problem}", name(case)));
        }
    }
    eprintln!(
        "{dir} fixtures: {} run, {} failed",
        cases.len(),
        failures.len()
    );
    assert!(failures.is_empty(), "\n{}", failures.join("\n\n"));
}

#[derive(Debug, PartialEq, Eq)]
enum Status {
    Pass,
    Pending,
    Fail(String),
}

fn spec_status(case: &Path) -> Status {
    let meta = case_toml(case);
    let result = run(case);
    let pending = |diags: &[Diagnostic]| diags.iter().any(|d| d.code() == Code::E5001);
    match (meta.get("expect").map(String::as_str), &result.outcome) {
        (Some("css"), Ok(css)) => {
            let expected = fs::read_to_string(case.join("expected.css")).expect("expected.css");
            if *css == expected {
                Status::Pass
            } else {
                Status::Fail(format!("--- expected ---\n{expected}--- actual ---\n{css}"))
            }
        }
        (Some("error"), Err(diags)) => {
            if all_matched(&expected_diagnostics(case), diags, &result.sources) {
                Status::Pass
            } else if pending(diags) {
                Status::Pending
            } else {
                Status::Fail(render(diags, &result.sources))
            }
        }
        (Some("css"), Err(diags)) if pending(diags) => Status::Pending,
        (_, Err(diags)) => Status::Fail(render(diags, &result.sources)),
        (_, Ok(css)) => Status::Fail(format!("unexpected success:\n{css}")),
    }
}

#[test]
fn spec_fixtures() {
    let cases = fixtures("spec");
    assert_eq!(cases.len(), 35, "expected the 35 phase 01 fixtures");
    let mut passing = Vec::new();
    let mut pending = 0;
    let mut failures = Vec::new();
    for case in &cases {
        match spec_status(case) {
            Status::Pass => passing.push(name(case)),
            Status::Pending => pending += 1,
            Status::Fail(detail) => failures.push(format!("{}:\n{detail}", name(case))),
        }
    }
    eprintln!(
        "spec fixtures: {} pass, {pending} pending (E5001), {} fail",
        passing.len(),
        failures.len()
    );
    assert!(failures.is_empty(), "\n{}", failures.join("\n\n"));
    assert_eq!(passing, SPEC_PASSING, "update SPEC_PASSING deliberately");
}

#[test]
fn passing_spec_output_is_valid_css() {
    for case in SPEC_PASSING {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/spec")
            .join(case);
        if let Ok(css) = run(&dir).outcome {
            // 024 preserves hacks such as `*zoom` that CSS parsers reject;
            // only the parts the parser understands are checked there.
            if case.starts_with("024") {
                continue;
            }
            independent::validate(&css);
        }
    }
}
