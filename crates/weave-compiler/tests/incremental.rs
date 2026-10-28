//! Incremental builds with a [`ModuleCache`] (phase 09).
//!
//! A scripted sequence of project states — leaf and shared-dependency
//! edits, deletions, renames, missing-file recovery, relocation of sources,
//! changed limits and toggled source maps — is built twice at every step:
//! once with a cache kept across all steps and once without any cache. The
//! two builds must agree on everything a caller can observe: CSS, source
//! map (with contents), warnings, rendered diagnostics and dependencies.
//! Cache statistics are checked where they show that unchanged modules were
//! reused and changed ones were not.

#![allow(
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::unwrap_used
)]

use std::collections::BTreeMap;
use std::fmt::Write as _;

use weave_compiler::{
    CacheStats, Compilation, CompileOptions, JsonOptions, Limit, LoadedModule, ModuleCache,
    ModuleId, ModuleResolver, ResolveError, compile, compile_with_cache, resolve,
};

fn id(path: &str) -> ModuleId {
    ModuleId::new(path).unwrap()
}

/// An in-memory project whose files can be removed and whose display names
/// carry a configurable prefix (standing in for the module root).
#[derive(Clone, Default)]
struct Project {
    files: BTreeMap<ModuleId, Vec<u8>>,
    prefix: String,
}

impl Project {
    fn write(&mut self, path: &str, text: &str) {
        self.files.insert(id(path), text.as_bytes().to_vec());
    }

    fn remove(&mut self, path: &str) {
        self.files.remove(&id(path));
    }
}

impl ModuleResolver for Project {
    fn resolve(&mut self, importer: &ModuleId, specifier: &str) -> Result<ModuleId, ResolveError> {
        let target = resolve::join_load_path(importer, specifier)?;
        if self.files.contains_key(&target) {
            Ok(target)
        } else {
            Err(ResolveError::NotFound(format!(
                "module `{target}` not found"
            )))
        }
    }

    fn load(&mut self, id: &ModuleId, _max_bytes: u64) -> Result<LoadedModule, ResolveError> {
        let bytes = self
            .files
            .get(id)
            .ok_or_else(|| ResolveError::NotFound(format!("module `{id}` not found")))?;
        Ok(LoadedModule {
            display_name: format!("{}{id}", self.prefix),
            bytes: bytes.clone(),
        })
    }
}

/// Everything a caller can observe of a build, as text.
fn observe(compilation: &Compilation) -> String {
    let mut out = String::new();
    let json = JsonOptions {
        sources_content: true,
        ..JsonOptions::default()
    };
    match &compilation.outcome {
        Ok(output) => {
            let _ = writeln!(out, "ok\n{}", output.css);
            if let Some(map) = &output.source_map {
                let _ = writeln!(out, "map {}", map.to_json(&json));
            }
            for warning in &output.warnings {
                out.push_str(&warning.render(&compilation.sources));
            }
            let _ = writeln!(out, "deps {:?}", output.dependencies);
        }
        Err(diagnostics) => {
            out.push_str("failed\n");
            for diagnostic in diagnostics {
                out.push_str(&diagnostic.render(&compilation.sources));
            }
        }
    }
    let _ = writeln!(out, "graph {:?}", compilation.graph.modules());
    out
}

/// Builds `entry` with the shared cache and without one; asserts that both
/// agree and returns the observation and the cache statistics of this
/// build alone.
fn step(
    label: &str,
    project: &Project,
    entry: &str,
    options: &CompileOptions,
    cache: &mut ModuleCache,
) -> (String, CacheStats) {
    let before = cache.stats();
    let cached = compile_with_cache(&id(entry), &mut project.clone(), options, cache);
    let after = cache.stats();
    let fresh = compile(&id(entry), &mut project.clone(), options);
    let (cached, fresh) = (observe(&cached), observe(&fresh));
    assert_eq!(
        cached, fresh,
        "{label}: a cached build differs from an uncached one"
    );
    let stats = CacheStats {
        hits: after.hits - before.hits,
        misses: after.misses - before.misses,
    };
    (cached, stats)
}

fn counts(stats: CacheStats) -> (u64, u64) {
    (stats.hits, stats.misses)
}

const MAIN: &str = r#"load "./tokens.weave" as t;
load "./parts/button.weave" as button;
load "./parts/card.weave" as card;

rule .main { gap: #{t.space(2)}; }
"#;

const TOKENS: &str = "export bind unit = 4px;
export bind ink = #111111;

export fn space(n: number) -> length {
    yield unit * n;
}
";

const BUTTON: &str = r#"load "../tokens.weave" as t;

rule .button {
    color: #{t.ink};
    padding: #{t.space(1)};
}
"#;

const CARD: &str = r#"load "../tokens.weave" as t;

rule .card { margin: #{t.space(3)}; }
"#;

fn project() -> Project {
    let mut project = Project {
        prefix: "src/".to_string(),
        ..Project::default()
    };
    project.write("main.weave", MAIN);
    project.write("tokens.weave", TOKENS);
    project.write("parts/button.weave", BUTTON);
    project.write("parts/card.weave", CARD);
    project
}

#[test]
fn edits_reuse_unchanged_modules_and_never_serve_stale_output() {
    let mut project = project();
    let options = CompileOptions::default();
    let mut cache = ModuleCache::new();

    let (initial, stats) = step("initial", &project, "main.weave", &options, &mut cache);
    assert_eq!(counts(stats), (0, 4));
    assert!(initial.contains("  margin: 12px;"));

    let (_, stats) = step("no-op", &project, "main.weave", &options, &mut cache);
    assert_eq!(counts(stats), (4, 0));

    // A leaf edit re-parses only the leaf.
    project.write("parts/card.weave", &CARD.replace("space(3)", "space(5)"));
    let (leaf, stats) = step("leaf edit", &project, "main.weave", &options, &mut cache);
    assert_eq!(counts(stats), (3, 1));
    assert!(leaf.contains("  margin: 20px;"));

    // A shared-dependency edit re-parses one module but changes the output
    // of every module that uses it.
    project.write("tokens.weave", &TOKENS.replace("4px", "5px"));
    let (shared, stats) = step("shared edit", &project, "main.weave", &options, &mut cache);
    assert_eq!(counts(stats), (3, 1));
    for line in ["  gap: 10px;", "  padding: 5px;", "  margin: 25px;"] {
        assert!(shared.contains(line), "{line}");
    }

    // An edit that keeps the length of the file.
    project.write("tokens.weave", &TOKENS.replace("4px", "6px"));
    let (same_length, stats) = step("same length", &project, "main.weave", &options, &mut cache);
    assert_eq!(counts(stats), (3, 1));
    assert!(same_length.contains("  padding: 6px;"));
}

#[test]
fn deletion_rename_and_missing_file_recovery() {
    let mut project = project();
    let options = CompileOptions::default();
    let mut cache = ModuleCache::new();
    step("initial", &project, "main.weave", &options, &mut cache);

    project.remove("parts/card.weave");
    let (deleted, _) = step("deleted", &project, "main.weave", &options, &mut cache);
    assert!(deleted.starts_with("failed"));
    assert!(deleted.contains("E7001"));
    assert!(!cache.contains(&id("parts/card.weave")));

    // The missing module appears again.
    project.write("parts/card.weave", CARD);
    let (recovered, stats) = step("recovered", &project, "main.weave", &options, &mut cache);
    assert!(recovered.starts_with("ok"));
    assert_eq!(counts(stats), (3, 1));

    // Renamed: the old identity leaves the cache, the new one is parsed.
    project.remove("parts/card.weave");
    project.write("parts/panel.weave", CARD);
    project.write(
        "main.weave",
        &MAIN.replace("./parts/card.weave", "./parts/panel.weave"),
    );
    let (renamed, stats) = step("renamed", &project, "main.weave", &options, &mut cache);
    assert!(renamed.starts_with("ok"));
    assert_eq!(counts(stats), (2, 2));
    assert!(!cache.contains(&id("parts/card.weave")));
    assert!(cache.contains(&id("parts/panel.weave")));
    assert_eq!(cache.len(), 4);

    // A module that is no longer loaded leaves the cache too.
    project.write(
        "main.weave",
        &MAIN.replace("load \"./parts/card.weave\" as card;\n", ""),
    );
    step("unloaded", &project, "main.weave", &options, &mut cache);
    assert!(!cache.contains(&id("parts/panel.weave")));
    assert_eq!(cache.len(), 3);
}

#[test]
fn relocated_sources_get_correct_positions_and_names() {
    let mut project = project();
    let mut options = CompileOptions::default();
    options.source_map = true;
    let mut cache = ModuleCache::new();
    step("initial", &project, "main.weave", &options, &mut cache);

    // A new first `load` shifts every later module's file position: their
    // spans would name the wrong files, so they must not be reused.
    project.write("first.weave", "rule .first { order: 1; }\n");
    project.write(
        "main.weave",
        &format!("load \"./first.weave\" as first;\n{MAIN}"),
    );
    let (shifted, stats) = step("shifted", &project, "main.weave", &options, &mut cache);
    assert!(shifted.contains(".first {"));
    assert_eq!(
        stats.misses, 5,
        "every module after the change is re-parsed"
    );

    // The same files under another root: names in maps and diagnostics
    // follow the resolver, parsed trees are still reused.
    project.prefix = "elsewhere/".to_string();
    let (moved, stats) = step("moved root", &project, "main.weave", &options, &mut cache);
    assert_eq!(counts(stats), (5, 0));
    assert!(moved.contains("elsewhere/parts/card.weave"));
    assert!(!moved.contains("src/"));

    // A module moved to another directory, with the same text.
    project.remove("first.weave");
    project.write("lib/first.weave", "rule .first { order: 1; }\n");
    project.write(
        "main.weave",
        &format!("load \"./lib/first.weave\" as first;\n{MAIN}"),
    );
    let (relocated, _) = step("relocated", &project, "main.weave", &options, &mut cache);
    assert!(relocated.contains("elsewhere/lib/first.weave"));

    // Diagnostics point into the right file after relocation.
    project.write("lib/first.weave", "rule .first { order: #{missing}; }\n");
    let (broken, _) = step("broken", &project, "main.weave", &options, &mut cache);
    assert!(broken.contains("elsewhere/lib/first.weave:1:"), "{broken}");
}

#[test]
fn options_and_limits_are_part_of_the_key() {
    let mut project = project();
    let mut cache = ModuleCache::new();
    let plain = CompileOptions::default();
    step("plain", &project, "main.weave", &plain, &mut cache);

    // Source maps on and off again: built from each build's own sources.
    let mut mapped = CompileOptions::default();
    mapped.source_map = true;
    let (with_map, stats) = step("map on", &project, "main.weave", &mapped, &mut cache);
    assert!(with_map.contains("map {\"version\":3"));
    assert_eq!(counts(stats), (4, 0));
    let (without, _) = step("map off", &project, "main.weave", &plain, &mut cache);
    assert!(!without.contains("map {"));

    // The nesting limit is applied by the parser: a lower limit re-parses
    // and reports E8002 even though the text is unchanged.
    project.write(
        "parts/card.weave",
        &CARD.replace(
            "rule .card {",
            "rule .card { rule .x { rule .y { order: 1; } }",
        ),
    );
    step("deeper", &project, "main.weave", &plain, &mut cache);
    let mut shallow = CompileOptions::default();
    shallow.limits.set(Limit::NestingDepth, 2).unwrap();
    let (too_deep, stats) = step("shallow", &project, "main.weave", &shallow, &mut cache);
    assert!(too_deep.contains("E8002"), "{too_deep}");
    assert_eq!(stats.hits, 0);
    let (restored, _) = step("restored", &project, "main.weave", &plain, &mut cache);
    assert!(restored.starts_with("ok"));

    // Limits checked before parsing apply to cached modules as well.
    let mut small_files = CompileOptions::default();
    small_files.limits.set(Limit::FileBytes, 64).unwrap();
    let (too_big, _) = step(
        "file bytes",
        &project,
        "main.weave",
        &small_files,
        &mut cache,
    );
    assert!(too_big.contains("E8001"));
    let mut small_total = CompileOptions::default();
    small_total.limits.set(Limit::TotalInputBytes, 300).unwrap();
    let (too_much, _) = step(
        "total bytes",
        &project,
        "main.weave",
        &small_total,
        &mut cache,
    );
    assert!(too_much.contains("E8001"));
    let mut tiny_output = CompileOptions::default();
    tiny_output.limits.set(Limit::OutputBytes, 10).unwrap();
    let (too_long, _) = step("output", &project, "main.weave", &tiny_output, &mut cache);
    assert!(too_long.contains("E8006"));
    let (back, _) = step("back", &project, "main.weave", &plain, &mut cache);
    assert!(back.starts_with("ok"));
}

#[test]
fn errors_roles_and_copies_are_never_shared() {
    let mut project = project();
    let options = CompileOptions::default();
    let mut cache = ModuleCache::new();
    step("initial", &project, "main.weave", &options, &mut cache);

    // A parse error is reported on every build until it is fixed.
    project.write("parts/button.weave", &BUTTON.replace("color:", "color"));
    for round in 0..2 {
        let (broken, _) = step("broken", &project, "main.weave", &options, &mut cache);
        assert!(broken.starts_with("failed"), "round {round}");
    }
    assert!(!cache.contains(&id("parts/button.weave")));
    project.write("parts/button.weave", BUTTON);
    let (fixed, _) = step("fixed", &project, "main.weave", &options, &mut cache);
    assert!(fixed.starts_with("ok"));

    // A module built as the entry, then as a dependency again: only the
    // entry may start with `@charset`. The role is part of the key; the
    // entry is also always the first file, so its file position changes
    // with its role as well.
    project.write(
        "tokens.weave",
        &format!("@charset \"UTF-8\";\n{TOKENS}rule .t {{ order: 1; }}\n"),
    );
    let (as_entry, _) = step("as entry", &project, "tokens.weave", &options, &mut cache);
    assert!(as_entry.starts_with("ok"));
    let (as_dependency, _) = step(
        "as dependency",
        &project,
        "main.weave",
        &options,
        &mut cache,
    );
    assert!(as_dependency.contains("E6008"), "{as_dependency}");

    // Identical text under two identities is two modules.
    project.write("tokens.weave", TOKENS);
    project.write("parts/card.weave", BUTTON);
    let (copies, _) = step("copies", &project, "main.weave", &options, &mut cache);
    assert_eq!(copies.matches(".button {").count(), 2);
}

#[test]
fn many_steps_of_random_edits_agree_with_uncached_builds() {
    // A deterministic pseudo-random walk over edits of every module.
    let mut project = project();
    let options = CompileOptions::default();
    let mut cache = ModuleCache::new();
    let mut state: u64 = 0x2545_f491_4f6c_dd1d;
    let paths = [
        "main.weave",
        "tokens.weave",
        "parts/button.weave",
        "parts/card.weave",
    ];
    let (mut successes, mut failures) = (0, 0);
    for round in 0..60 {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        let path = paths[(state % 4) as usize];
        let base = match path {
            "main.weave" => MAIN,
            "tokens.weave" => TOKENS,
            "parts/button.weave" => BUTTON,
            _ => CARD,
        };
        match (state >> 8) % 8 {
            0 => project.remove(path),
            1 => project.write(path, &format!("{base}rule .broken {{")),
            2..=4 => project.write(
                path,
                &format!("{base}rule .r{round} {{ order: {round}; }}\n"),
            ),
            _ => project.write(path, base),
        }
        let (seen, _) = step(
            &format!("round {round}"),
            &project,
            "main.weave",
            &options,
            &mut cache,
        );
        if seen.starts_with("ok") {
            successes += 1;
        } else {
            failures += 1;
        }
    }
    // The walk visits both working and broken states.
    assert!(
        successes >= 10 && failures >= 10,
        "{successes} ok, {failures} failed"
    );
}
