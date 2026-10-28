//! `weave watch`: rebuild whenever an input changes.
//!
//! The watcher polls file metadata (existence, length, modification time)
//! at a fixed interval, using only the standard library, so it behaves the
//! same on every platform. The watched set is recomputed after every build
//! from what that build touched: every loaded module, the entry, and the
//! paths of `load` statements that failed to resolve, so creating a missing
//! module, deleting or renaming one, or editing `load` statements all
//! trigger the right rebuilds. Output files are never watched, and outputs
//! with the `.weave` extension or naming an input are refused, so writing
//! output cannot cause a rebuild loop.
//!
//! A failed build writes nothing: the last successful output stays in
//! place until a build succeeds again.
//!
//! Limitations: a change that keeps a file's length and lands within the
//! file system's timestamp granularity (for example two writes in the same
//! second on a file system with one-second timestamps) can go unnoticed
//! until the next change; changes during a build are detected against the
//! state seen before that build; files first read by a build are recorded
//! right after it, before it is reported.

use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::fs;
use std::io::{self, Write};
use std::path::PathBuf;
use std::time::SystemTime;

use crate::args::{DiagnosticFormat, Target};
use crate::pipeline::Session;

/// What polling observes of one path.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Stamp {
    Missing,
    File {
        len: u64,
        modified: Option<SystemTime>,
    },
}

fn stamp(path: &PathBuf) -> Stamp {
    match fs::metadata(path) {
        Ok(meta) => Stamp::File {
            len: meta.len(),
            modified: meta.modified().ok(),
        },
        Err(_) => Stamp::Missing,
    }
}

fn snapshot(paths: &BTreeSet<PathBuf>) -> BTreeMap<PathBuf, Stamp> {
    paths
        .iter()
        .map(|path| (path.clone(), stamp(path)))
        .collect()
}

/// Runs builds forever; returns only if writing status fails.
pub fn run(session: &mut Session) -> io::Result<()> {
    let output = match &session.options.output {
        Some(Target::File(path)) => path.display().to_string(),
        _ => "-".to_string(),
    };
    let human = session.options.diagnostics == DiagnosticFormat::Human;
    let mut watched: BTreeSet<PathBuf> = BTreeSet::new();
    let mut announced = false;
    loop {
        // The state before the build is the baseline for files the build
        // reads, so a change made while it runs triggers another build.
        let before = snapshot(&watched);
        let outcome = session.run();
        let mut baseline = BTreeMap::new();
        for path in &outcome.watched {
            let seen = before.get(path).copied().unwrap_or_else(|| stamp(path));
            baseline.insert(path.clone(), seen);
        }
        watched = outcome.watched;
        let mut stderr = io::stderr().lock();
        stderr.write_all(outcome.report.as_bytes())?;
        if human {
            if outcome.success {
                writeln!(
                    stderr,
                    "[weave] wrote {output} ({} bytes from {} module{})",
                    outcome.css_bytes,
                    outcome.modules,
                    if outcome.modules == 1 { "" } else { "s" }
                )?;
            } else {
                writeln!(
                    stderr,
                    "[weave] build failed; {output} keeps the last successful output"
                )?;
            }
            if !announced {
                writeln!(
                    stderr,
                    "[weave] watching for changes every {} ms (Ctrl-C to stop)",
                    session.options.poll_interval.as_millis()
                )?;
                announced = true;
            }
        }
        stderr.flush()?;
        drop(stderr);
        loop {
            std::thread::sleep(session.options.poll_interval);
            if baseline.iter().any(|(path, seen)| stamp(path) != *seen) {
                break;
            }
        }
    }
}
