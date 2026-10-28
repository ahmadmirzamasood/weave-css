//! `weave-bench`: benchmark workloads and in-process measurements.
//!
//! ```text
//! weave-bench list
//! weave-bench generate <dir>
//! weave-bench run [--samples N] [--warmup N] [--workload NAME]... [--out FILE]
//! ```
//!
//! `generate` writes the canonical corpus (one directory per workload) for
//! process-level measurements (scripts/bench.py). `run` measures in-process
//! builds and prints raw samples as JSON. Its scenarios:
//!
//! - `warm-fresh`: a new [`ModuleCache`] per build, so every module is read
//!   and parsed; the process, allocator and file contents are already warm.
//!   This is not a cold measurement.
//! - `warm-fresh-map`: the same with source maps.
//! - `warm-unchanged`: rebuilding unchanged sources with a primed cache.
//! - `edit-<label>`: each sample changes one module (a new revision every
//!   time) and rebuilds with the cache of the previous build.
//!
//! Every build is checked against the workload's expectations, and every
//! cached build is compared byte for byte with an uncached build of the same
//! sources; any difference aborts the run.

#![forbid(unsafe_code)]

mod workloads;

use std::fmt::Write as _;
use std::process::ExitCode;
use std::time::Instant;

use weave_compiler::{
    CompileOptions, MemoryResolver, ModuleCache, ModuleId, Phase, compile_observed,
};

use workloads::Workload;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("list") => {
            for workload in workloads::all() {
                println!(
                    "{}\t{} modules\t{} bytes\t{}",
                    workload.name,
                    workload.files.len(),
                    workload.input_bytes(),
                    workload.description
                );
            }
            Ok(())
        }
        Some("generate") => match args.get(1) {
            Some(dir) => generate(std::path::Path::new(dir)),
            None => Err("usage: weave-bench generate <dir>".to_string()),
        },
        Some("run") => parse_run(args.get(1..).unwrap_or_default()).and_then(|config| run(&config)),
        _ => Err("usage: weave-bench list | generate <dir> | run [--samples N] [--warmup N] [--workload NAME]... [--out FILE]".to_string()),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("weave-bench: {message}");
            ExitCode::FAILURE
        }
    }
}

fn generate(dir: &std::path::Path) -> Result<(), String> {
    for workload in workloads::all() {
        for (path, template) in &workload.files {
            let target = dir.join(workload.name).join(path);
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent)
                    .map_err(|e| format!("{}: {e}", parent.display()))?;
            }
            std::fs::write(&target, workload.render(template, 0))
                .map_err(|e| format!("{}: {e}", target.display()))?;
        }
        println!("{}\t{}", workload.name, workload.entry());
    }
    Ok(())
}

#[derive(Debug)]
struct RunConfig {
    samples: usize,
    warmup: usize,
    only: Vec<String>,
    out: Option<String>,
}

fn parse_run(args: &[String]) -> Result<RunConfig, String> {
    let mut config = RunConfig {
        samples: 30,
        warmup: 3,
        only: Vec::new(),
        out: None,
    };
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        let mut value = || {
            iter.next()
                .cloned()
                .ok_or_else(|| format!("`{arg}` needs a value"))
        };
        match arg.as_str() {
            "--samples" => config.samples = number(&value()?)?,
            "--warmup" => config.warmup = number(&value()?)?,
            "--workload" => config.only.push(value()?),
            "--out" => config.out = Some(value()?),
            other => return Err(format!("unknown option `{other}`")),
        }
    }
    if config.samples == 0 {
        return Err("--samples must be at least 1".to_string());
    }
    Ok(config)
}

fn number(text: &str) -> Result<usize, String> {
    text.parse()
        .map_err(|_| format!("`{text}` is not a number"))
}

/// One timed build: total and per-phase nanoseconds.
#[derive(Clone, Copy, Debug, Default)]
struct Sample {
    total: u64,
    load: u64,
    eval: u64,
    emit: u64,
}

#[derive(Debug)]
struct Scenario {
    name: String,
    samples: Vec<Sample>,
    /// Parse-cache hits and misses of the last sample.
    hits: u64,
    misses: u64,
}

#[derive(Debug)]
struct Measured {
    workload: &'static str,
    modules: usize,
    input_bytes: usize,
    output_bytes: usize,
    output_fnv1a64: u64,
    map_bytes: usize,
    scenarios: Vec<Scenario>,
}

/// The sources of a workload at some revision.
struct Sources<'w> {
    workload: &'w Workload,
    /// Edit counter per module.
    counters: Vec<u64>,
}

impl Sources<'_> {
    fn resolver(&self) -> Result<MemoryResolver, String> {
        let mut resolver = MemoryResolver::new();
        for ((path, template), counter) in self.workload.files.iter().zip(&self.counters) {
            let id = ModuleId::new(path.clone()).map_err(|e| e.to_string())?;
            resolver.insert(id, self.workload.render(template, *counter));
        }
        Ok(resolver)
    }
}

/// Builds once, returning the CSS (and map JSON length) and timing.
fn build(
    sources: &Sources<'_>,
    options: &CompileOptions,
    cache: &mut ModuleCache,
) -> Result<(Sample, String, usize), String> {
    let entry = ModuleId::new(sources.workload.entry()).map_err(|e| e.to_string())?;
    let mut resolver = sources.resolver()?;
    let mut marks = [0u64; 3];
    let start = Instant::now();
    let compilation = compile_observed(&entry, &mut resolver, options, cache, &mut |phase| {
        let at = nanos(start);
        let slot = match phase {
            Phase::Loaded => 0,
            Phase::Evaluated => 1,
            Phase::Emitted => 2,
        };
        if let Some(mark) = marks.get_mut(slot) {
            *mark = at;
        }
    });
    let total = nanos(start);
    let output = compilation.outcome.map_err(|diagnostics| {
        format!(
            "{}: build failed: {:?}",
            sources.workload.name,
            diagnostics
                .iter()
                .map(|d| format!("{} {}", d.code(), d.message()))
                .collect::<Vec<_>>()
        )
    })?;
    let [load, evaluated, emitted] = marks;
    let sample = Sample {
        total,
        load,
        eval: evaluated.saturating_sub(load),
        emit: emitted.saturating_sub(evaluated),
    };
    let map_bytes = output.source_map.as_ref().map_or(0, |map| {
        map.to_json(&weave_compiler::JsonOptions::default()).len()
    });
    Ok((sample, output.css, map_bytes))
}

fn nanos(start: Instant) -> u64 {
    u64::try_from(start.elapsed().as_nanos()).unwrap_or(u64::MAX)
}

fn run(config: &RunConfig) -> Result<(), String> {
    let mut results = Vec::new();
    for workload in workloads::all() {
        if !config.only.is_empty() && !config.only.iter().any(|n| n == workload.name) {
            continue;
        }
        eprintln!("measuring {}", workload.name);
        results.push(measure(&workload, config)?);
    }
    let json = to_json(&results, config);
    match &config.out {
        Some(path) => std::fs::write(path, json).map_err(|e| format!("{path}: {e}")),
        None => {
            println!("{json}");
            Ok(())
        }
    }
}

fn measure(workload: &Workload, config: &RunConfig) -> Result<Measured, String> {
    let plain = CompileOptions::default();
    let mut mapped = CompileOptions::default();
    mapped.source_map = true;
    let canonical = Sources {
        workload,
        counters: vec![0; workload.files.len()],
    };

    // Reference output and correctness check.
    let (_, reference, _) = build(&canonical, &plain, &mut ModuleCache::new())?;
    workload.check(&reference)?;
    let (_, mapped_css, map_bytes) = build(&canonical, &mapped, &mut ModuleCache::new())?;
    if mapped_css != reference {
        return Err(format!("{}: source maps changed the CSS", workload.name));
    }

    let mut scenarios = Vec::new();
    for (name, options) in [("warm-fresh", &plain), ("warm-fresh-map", &mapped)] {
        let mut scenario = Scenario {
            name: name.to_string(),
            samples: Vec::new(),
            hits: 0,
            misses: 0,
        };
        for round in 0..config.warmup + config.samples {
            let mut cache = ModuleCache::new();
            let (sample, css, _) = build(&canonical, options, &mut cache)?;
            if css != reference {
                return Err(format!("{}: {name} output differs", workload.name));
            }
            scenario.hits = cache.stats().hits;
            scenario.misses = cache.stats().misses;
            if round >= config.warmup {
                scenario.samples.push(sample);
            }
        }
        scenarios.push(scenario);
    }

    // Unchanged rebuilds with a primed cache.
    let mut cache = ModuleCache::new();
    build(&canonical, &plain, &mut cache)?;
    let mut scenario = Scenario {
        name: "warm-unchanged".to_string(),
        samples: Vec::new(),
        hits: 0,
        misses: 0,
    };
    for round in 0..config.warmup + config.samples {
        let before = cache.stats();
        let (sample, css, _) = build(&canonical, &plain, &mut cache)?;
        if css != reference {
            return Err(format!("{}: cached output differs", workload.name));
        }
        scenario.hits = cache.stats().hits - before.hits;
        scenario.misses = cache.stats().misses - before.misses;
        if round >= config.warmup {
            scenario.samples.push(sample);
        }
    }
    scenarios.push(scenario);

    // Incremental edits: a new revision of one module per sample.
    for (label, path) in &workload.edits {
        let position = workload
            .files
            .iter()
            .position(|(p, _)| p == path)
            .ok_or_else(|| format!("{}: no module {path}", workload.name))?;
        let mut sources = Sources {
            workload,
            counters: vec![0; workload.files.len()],
        };
        let mut cache = ModuleCache::new();
        build(&sources, &plain, &mut cache)?;
        let mut scenario = Scenario {
            name: format!("edit-{label}"),
            samples: Vec::new(),
            hits: 0,
            misses: 0,
        };
        for round in 0..config.warmup + config.samples {
            if let Some(counter) = sources.counters.get_mut(position) {
                *counter = u64::try_from(round).unwrap_or(u64::MAX) + 1;
            }
            let before = cache.stats();
            let (sample, css, _) = build(&sources, &plain, &mut cache)?;
            scenario.hits = cache.stats().hits - before.hits;
            scenario.misses = cache.stats().misses - before.misses;
            // Untimed: the same revision built without any cache.
            let (_, fresh, _) = build(&sources, &plain, &mut ModuleCache::new())?;
            if css != fresh {
                return Err(format!(
                    "{}: edit-{label} round {round}: cached output differs from an uncached build",
                    workload.name
                ));
            }
            workload.check(&css)?;
            if round >= config.warmup {
                scenario.samples.push(sample);
            }
        }
        scenarios.push(scenario);
    }

    Ok(Measured {
        workload: workload.name,
        modules: workload.files.len(),
        input_bytes: workload.input_bytes(),
        output_bytes: reference.len(),
        output_fnv1a64: fnv1a64(reference.as_bytes()),
        map_bytes,
        scenarios,
    })
}

fn fnv1a64(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, &byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3)
    })
}

fn to_json(results: &[Measured], config: &RunConfig) -> String {
    let mut out = String::new();
    let _ = write!(
        out,
        "{{\"tool\":\"weave-bench\",\"compiler_version\":\"{}\",\"generator_version\":{},\"samples\":{},\"warmup\":{},\"debug_assertions\":{},\"unit\":\"ns\",\"workloads\":[",
        weave_compiler::VERSION,
        workloads::GENERATOR_VERSION,
        config.samples,
        config.warmup,
        cfg!(debug_assertions),
    );
    for (i, result) in results.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        let _ = write!(
            out,
            "{{\"name\":\"{}\",\"modules\":{},\"input_bytes\":{},\"output_bytes\":{},\"output_fnv1a64\":\"{:016x}\",\"map_bytes\":{},\"scenarios\":[",
            result.workload,
            result.modules,
            result.input_bytes,
            result.output_bytes,
            result.output_fnv1a64,
            result.map_bytes,
        );
        for (j, scenario) in result.scenarios.iter().enumerate() {
            if j > 0 {
                out.push(',');
            }
            let list = |f: fn(&Sample) -> u64| {
                scenario
                    .samples
                    .iter()
                    .map(|s| f(s).to_string())
                    .collect::<Vec<_>>()
                    .join(",")
            };
            let _ = write!(
                out,
                "{{\"name\":\"{}\",\"cache_hits\":{},\"cache_misses\":{},\"total\":[{}],\"load\":[{}],\"eval\":[{}],\"emit\":[{}]}}",
                scenario.name,
                scenario.hits,
                scenario.misses,
                list(|s| s.total),
                list(|s| s.load),
                list(|s| s.eval),
                list(|s| s.emit),
            );
        }
        out.push_str("]}");
    }
    out.push_str("]}");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every workload compiles and meets its own expectations; edits keep
    /// it valid. Uses a debug build, so it also guards the generators.
    #[test]
    fn workloads_compile_and_check() {
        for workload in workloads::all() {
            let mut sources = Sources {
                workload: &workload,
                counters: vec![0; workload.files.len()],
            };
            let (_, css, _) = build(
                &sources,
                &CompileOptions::default(),
                &mut ModuleCache::new(),
            )
            .unwrap();
            workload.check(&css).unwrap();
            sources.counters.fill(7);
            let (_, edited, _) = build(
                &sources,
                &CompileOptions::default(),
                &mut ModuleCache::new(),
            )
            .unwrap();
            workload.check(&edited).unwrap();
            assert_ne!(css, edited, "{}", workload.name);
        }
    }

    #[test]
    fn generators_are_deterministic() {
        let first: Vec<_> = workloads::all().into_iter().map(|w| w.files).collect();
        let second: Vec<_> = workloads::all().into_iter().map(|w| w.files).collect();
        assert_eq!(first, second);
    }
}
