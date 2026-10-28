//! `weave watch`: rebuilds on change, keeps the last good output after a
//! failed build, follows module creation, deletion, renaming and graph
//! changes, and never rebuilds because of its own output.

#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::time::{Duration, Instant};

const TIMEOUT: Duration = Duration::from_secs(20);

fn scratch(name: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("watch-{name}"));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("create scratch directory");
    dir
}

/// Writes a file atomically, so the watcher never sees a partial write.
fn put(path: &Path, text: &str) {
    let temp = path.with_extension("tmp-test");
    fs::write(&temp, text).unwrap();
    fs::rename(&temp, path).unwrap();
}

/// A running `weave watch` whose JSON build reports arrive on a channel.
struct Watcher {
    child: Child,
    reports: Receiver<String>,
    dir: PathBuf,
}

impl Watcher {
    fn start(dir: &Path, extra: &[&str]) -> Watcher {
        let mut child = Command::new(env!("CARGO_BIN_EXE_weave"))
            .current_dir(dir)
            .args([
                "watch",
                "main.weave",
                "-o",
                "out.css",
                "--poll-interval",
                "20",
            ])
            .args(["--diagnostics", "json"])
            .args(extra)
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .expect("failed to start weave watch");
        let stderr = child.stderr.take().unwrap();
        let (send, reports) = mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stderr).lines() {
                let Ok(line) = line else { break };
                if send.send(line).is_err() {
                    break;
                }
            }
        });
        Watcher {
            child,
            reports,
            dir: dir.to_path_buf(),
        }
    }

    #[track_caller]
    /// The next build report.
    fn next(&self) -> String {
        match self.reports.recv_timeout(TIMEOUT) {
            Ok(line) => {
                assert!(
                    line.starts_with("{\"version\":1,\"command\":\"watch\""),
                    "{line}"
                );
                line
            }
            Err(err) => panic!("no build report: {err:?}"),
        }
    }

    #[track_caller]
    /// Waits for a successful build.
    fn success(&self) -> String {
        let line = self.next();
        assert!(
            line.contains("\"success\":true"),
            "expected success: {line}"
        );
        line
    }

    #[track_caller]
    /// Waits for a failed build reporting `code`.
    fn failure(&self, code: &str) -> String {
        let line = self.next();
        assert!(
            line.contains("\"success\":false"),
            "expected failure: {line}"
        );
        assert!(
            line.contains(&format!("\"code\":\"{code}\"")),
            "expected {code}: {line}"
        );
        line
    }

    /// Asserts that no build happens for a while.
    fn quiet(&self, wait: Duration) {
        match self.reports.recv_timeout(wait) {
            Err(RecvTimeoutError::Timeout) => {}
            other => panic!("unexpected build: {other:?}"),
        }
    }

    fn css(&self) -> String {
        fs::read_to_string(self.dir.join("out.css")).unwrap()
    }
}

impl Drop for Watcher {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[test]
fn rebuilds_and_keeps_the_last_good_output() {
    let dir = scratch("recover");
    put(&dir.join("main.weave"), "rule a { b: c; }\n");
    let watcher = Watcher::start(&dir, &[]);
    watcher.success();
    assert_eq!(watcher.css(), "a {\n  b: c;\n}\n");

    put(&dir.join("main.weave"), "rule a { b: d; }\n");
    watcher.success();
    assert_eq!(watcher.css(), "a {\n  b: d;\n}\n");

    // A failed build reports diagnostics and leaves the output alone.
    put(&dir.join("main.weave"), "rule a { b: #{nope}; }\n");
    watcher.failure("E1002");
    assert_eq!(watcher.css(), "a {\n  b: d;\n}\n");

    // Recovery writes the new output.
    put(&dir.join("main.weave"), "rule a { b: e; }\n");
    watcher.success();
    assert_eq!(watcher.css(), "a {\n  b: e;\n}\n");

    // Writing the output does not trigger another build; no temporary
    // files remain.
    watcher.quiet(Duration::from_millis(400));
    let mut names: Vec<_> = fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    assert_eq!(names, ["main.weave", "out.css"]);
}

#[test]
fn follows_module_creation_deletion_and_renaming() {
    let dir = scratch("modules");
    put(
        &dir.join("main.weave"),
        "load \"./a.weave\" as a;\nrule m { gap: #{a.g}; }\n",
    );
    let watcher = Watcher::start(&dir, &[]);
    // The module is missing: the failure still watches its path.
    watcher.failure("E7001");
    assert!(!dir.join("out.css").exists());

    put(&dir.join("a.weave"), "export bind g = 1px;\n");
    watcher.success();
    assert_eq!(watcher.css(), "m {\n  gap: 1px;\n}\n");

    // Editing the dependency rebuilds.
    put(&dir.join("a.weave"), "export bind g = 2px;\n");
    watcher.success();
    assert_eq!(watcher.css(), "m {\n  gap: 2px;\n}\n");

    // Deleting it fails and keeps the output.
    fs::rename(dir.join("a.weave"), dir.join("b.weave")).unwrap();
    watcher.failure("E7001");
    assert_eq!(watcher.css(), "m {\n  gap: 2px;\n}\n");

    // Following the rename in the entry recovers and switches the graph.
    put(
        &dir.join("main.weave"),
        "load \"./b.weave\" as a;\nrule m { gap: #{a.g}; }\n",
    );
    let report = watcher.success();
    assert!(
        report.contains("\"dependencies\":[\"main.weave\",\"b.weave\"]"),
        "{report}"
    );

    // The old path is no longer part of the graph.
    put(&dir.join("a.weave"), "export bind g = 9px;\n");
    watcher.quiet(Duration::from_millis(400));

    // Renaming back restores the original graph's input.
    fs::remove_file(dir.join("b.weave")).unwrap();
    watcher.failure("E7001");
    put(
        &dir.join("main.weave"),
        "load \"./a.weave\" as a;\nrule m { gap: #{a.g}; }\n",
    );
    watcher.success();
    assert_eq!(watcher.css(), "m {\n  gap: 9px;\n}\n");
}

#[test]
fn graph_changes_add_and_drop_watched_modules() {
    let dir = scratch("graph");
    fs::create_dir_all(dir.join("lib")).unwrap();
    put(&dir.join("main.weave"), "rule m { a: b; }\n");
    put(&dir.join("lib/x.weave"), "rule x { a: b; }\n");
    let watcher = Watcher::start(&dir, &["--source-map", "--deps", "out.d"]);
    watcher.success();
    // Unrelated files are not watched.
    put(&dir.join("lib/x.weave"), "rule x { a: c; }\n");
    watcher.quiet(Duration::from_millis(300));

    // A new load adds the module to the watched set.
    put(
        &dir.join("main.weave"),
        "load \"./lib/x.weave\" as x;\nrule m { a: b; }\n",
    );
    watcher.success();
    assert_eq!(
        watcher.css(),
        "x {\n  a: c;\n}\n\nm {\n  a: b;\n}\n/*# sourceMappingURL=out.css.map */\n"
    );
    assert_eq!(
        fs::read_to_string(dir.join("out.d")).unwrap(),
        "main.weave\nlib/x.weave\n"
    );
    put(&dir.join("lib/x.weave"), "rule x { a: d; }\n");
    watcher.success();
    assert!(watcher.css().starts_with("x {\n  a: d;\n}"));

    // Generated map and dependency files never trigger a build.
    watcher.quiet(Duration::from_millis(300));
    assert!(dir.join("out.css.map").is_file());
}

#[test]
fn deleted_entry_fails_and_recovers() {
    let dir = scratch("entry");
    put(&dir.join("main.weave"), "rule a { b: c; }\n");
    let watcher = Watcher::start(&dir, &[]);
    watcher.success();
    fs::remove_file(dir.join("main.weave")).unwrap();
    watcher.failure("E7001");
    assert_eq!(watcher.css(), "a {\n  b: c;\n}\n");
    put(&dir.join("main.weave"), "rule a { b: z; }\n");
    watcher.success();
    assert_eq!(watcher.css(), "a {\n  b: z;\n}\n");
}

#[test]
fn human_status_lines() {
    let dir = scratch("human");
    put(&dir.join("main.weave"), "rule a { b: c; }\n");
    let mut child = Command::new(env!("CARGO_BIN_EXE_weave"))
        .current_dir(&dir)
        .args([
            "watch",
            "main.weave",
            "-o",
            "out.css",
            "--poll-interval",
            "20",
        ])
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut lines = BufReader::new(child.stderr.take().unwrap()).lines();
    let deadline = Instant::now() + TIMEOUT;
    let first = lines.next().unwrap().unwrap();
    assert_eq!(first, "[weave] wrote out.css (14 bytes from 1 module)");
    let second = lines.next().unwrap().unwrap();
    assert_eq!(
        second,
        "[weave] watching for changes every 20 ms (Ctrl-C to stop)"
    );
    put(&dir.join("main.weave"), "rule a { b: #{x}; }\n");
    let error = lines.next().unwrap().unwrap();
    assert!(error.starts_with("error[E1002]"), "{error}");
    let mut status = String::new();
    while Instant::now() < deadline {
        status = lines.next().unwrap().unwrap();
        if status.starts_with("[weave]") {
            break;
        }
    }
    assert_eq!(
        status,
        "[weave] build failed; out.css keeps the last successful output"
    );
    let _ = child.kill();
    let _ = child.wait();
}
