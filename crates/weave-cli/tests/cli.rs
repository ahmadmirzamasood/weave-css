//! End-to-end tests of the `weave` binary.

// Helpers outside `#[test]` functions fail loudly on harness errors.
#![allow(clippy::expect_used)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn weave(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_weave"))
        .args(args)
        .output()
        .expect("failed to run weave")
}

fn stdout(output: &Output) -> String {
    String::from_utf8(output.stdout.clone()).expect("stdout is UTF-8")
}

fn stderr(output: &Output) -> String {
    String::from_utf8(output.stderr.clone()).expect("stderr is UTF-8")
}

fn scratch(name: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("cli-{name}"));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("create scratch directory");
    dir
}

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

#[test]
fn version_reports_crate_and_language_versions() {
    for flag in ["--version", "-V", "version"] {
        let out = weave(&[flag]);
        assert_eq!(out.status.code(), Some(0), "{flag}");
        let text = stdout(&out);
        assert!(
            text.starts_with(&format!("weave {}\n", env!("CARGO_PKG_VERSION"))),
            "{text}"
        );
        assert!(text.contains("language specification: v0.1"));
        assert!(text.contains("compilation: not implemented"));
        assert!(out.stderr.is_empty());
    }
}

#[test]
fn help_lists_commands_and_states_status() {
    for flag in ["--help", "-h", "help"] {
        let out = weave(&[flag]);
        assert_eq!(out.status.code(), Some(0), "{flag}");
        let text = stdout(&out);
        for needle in [
            "weave build <input.weave>",
            "weave check <input.weave>",
            "E5001",
        ] {
            assert!(text.contains(needle), "{flag}: missing {needle}");
        }
        assert!(text.contains("not implemented"));
    }
}

#[test]
fn invalid_command_lines_exit_with_usage_status() {
    for args in [
        &[][..],
        &["frobnicate"],
        &["build"],
        &["check", "--watch", "a.weave"],
    ] {
        let out = weave(args);
        assert_eq!(out.status.code(), Some(2), "{args:?}");
        assert!(out.stdout.is_empty());
        assert!(stderr(&out).contains("weave --help"), "{args:?}");
    }
}

#[test]
fn build_reports_unsupported_and_writes_no_css() {
    let dir = scratch("unsupported");
    let input = dir.join("main.weave");
    fs::write(&input, "rule a {\n  color: red;\n}\n").expect("write input");
    for command in ["build", "check"] {
        let out = weave(&[command, input.to_str().expect("UTF-8 path")]);
        assert_eq!(out.status.code(), Some(1), "{command}");
        assert!(out.stdout.is_empty(), "{command} must not write CSS");
        let err = stderr(&out);
        assert!(err.starts_with("error[E5001]: "), "{err}");
        assert!(err.contains("`main.weave` was read"), "{err}");
        // The absolute scratch path is not leaked into diagnostics.
        assert!(!err.contains(dir.to_str().expect("UTF-8 path")), "{err}");
    }
}

#[test]
fn source_text_errors_report_unicode_positions() {
    let dir = scratch("utf8");
    let input = dir.join("main.weave");
    // Line 2 is `é→` followed by an invalid byte at column 3.
    fs::write(&input, b"\xef\xbb\xbfa\r\n\xc3\xa9\xe2\x86\x92\xff\n").expect("write input");
    let out = weave(&["check", input.to_str().expect("UTF-8 path")]);
    assert_eq!(out.status.code(), Some(1));
    let err = stderr(&out);
    assert!(err.starts_with("error[E0001]: "), "{err}");
    assert!(err.contains("--> main.weave:2:3\n"), "{err}");

    fs::write(&input, "bind é = 1;\nbind x\0 = 2;\n").expect("write input");
    let out = weave(&["check", input.to_str().expect("UTF-8 path")]);
    let err = stderr(&out);
    assert!(err.starts_with("error[E0006]: "), "{err}");
    assert!(err.contains("--> main.weave:2:7\n"), "{err}");
}

#[test]
fn missing_input_is_reported() {
    let dir = scratch("missing");
    let input = dir.join("absent.weave");
    let out = weave(&["build", input.to_str().expect("UTF-8 path")]);
    assert_eq!(out.status.code(), Some(1));
    assert!(stderr(&out).starts_with("error[E7001]: input file"));
    assert!(out.stdout.is_empty());
}

#[test]
fn control_characters_in_paths_are_escaped() {
    let out = weave(&["build", "missing\u{1b}[2J.weave"]);
    assert_eq!(out.status.code(), Some(1));
    let err = stderr(&out);
    assert!(!err.contains('\u{1b}'), "{err:?}");
    assert!(err.contains("missing\\u{1b}[2J.weave"), "{err}");
}

/// Every specification fixture must currently fail with E5001 (or a source
/// text error) rather than produce output: no fixture is claimed to pass.
#[test]
fn specification_fixtures_are_not_compiled_yet() {
    let fixtures = repo_root().join("tests/fixtures/spec");
    let mut count = 0;
    for entry in fs::read_dir(&fixtures).expect("fixture directory") {
        let case = entry.expect("fixture entry").path();
        let main = case.join("main.weave");
        if !main.is_file() {
            continue;
        }
        count += 1;
        let out = weave(&["check", main.to_str().expect("UTF-8 path")]);
        assert_eq!(out.status.code(), Some(1), "{}", case.display());
        assert!(
            stderr(&out).starts_with("error[E5001]: "),
            "{}: {}",
            case.display(),
            stderr(&out)
        );
    }
    assert_eq!(count, 35, "expected the 35 phase 01 fixtures");
}
