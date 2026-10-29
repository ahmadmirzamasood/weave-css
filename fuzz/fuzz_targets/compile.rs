//! Whole builds of up to four in-memory modules.
//!
//! The first byte selects options; the rest is split at 0xFF bytes (never
//! part of UTF-8) into `main.weave`, `a.weave`, `lib/b.weave` and
//! `lib/c.weave`. Limits are lowered so that each run stays fast; hitting a
//! limit must still be a diagnostic, never a panic or hang.
//!
//! Checked: no "internal compiler error" (a broken evaluator invariant),
//! two uncached builds and a cached rebuild agree exactly (CSS,
//! source map, diagnostics), every diagnostic location resolves, rendered
//! diagnostics contain no control characters, and default source-map JSON
//! carries no source content.
#![no_main]

use libfuzzer_sys::fuzz_target;
use weave_compiler::{
    Compilation, CompileOptions, JsonOptions, Limit, MemoryResolver, ModuleCache, ModuleId,
    ResourceLimits, compile, compile_with_cache,
};

const NAMES: [&str; 4] = ["main.weave", "a.weave", "lib/b.weave", "lib/c.weave"];

fn resolver(parts: &[&[u8]]) -> MemoryResolver {
    let mut resolver = MemoryResolver::new();
    for (name, bytes) in NAMES.iter().zip(parts) {
        if let Ok(id) = ModuleId::new(*name) {
            resolver.insert(id, bytes.to_vec());
        }
    }
    resolver
}

fn options(flags: u8) -> CompileOptions {
    let mut limits = ResourceLimits::default();
    let lowered = [
        (Limit::EvaluationSteps, 50_000),
        (Limit::EmittedSelectors, 10_000),
        (Limit::OutputBytes, 1 << 20),
        (Limit::CollectionElements, 4096),
        (Limit::StringBytes, 1 << 16),
        (Limit::NestingDepth, if flags & 4 != 0 { 512 } else { 128 }),
        (Limit::CallDepth, if flags & 8 != 0 { 512 } else { 64 }),
        (Limit::Diagnostics, if flags & 16 != 0 { 3 } else { 100 }),
    ];
    for (limit, value) in lowered {
        let _ = limits.set(limit, value);
    }
    let mut options = CompileOptions::default();
    options.limits = limits;
    options.source_map = flags & 1 != 0;
    options
}

/// Everything observable about a build, for comparison.
fn summary(build: &Compilation) -> String {
    match &build.outcome {
        Ok(output) => {
            let map = output
                .source_map
                .as_ref()
                .map(|map| map.to_json(&JsonOptions::default()));
            let warnings: Vec<String> =
                output.warnings.iter().map(|d| d.render(&build.sources)).collect();
            format!("ok\n{}\n{map:?}\n{:?}\n{warnings:?}", output.css, output.dependencies)
        }
        Err(diagnostics) => {
            let rendered: Vec<String> = diagnostics.iter().map(|d| d.render(&build.sources)).collect();
            format!("err\n{rendered:?}")
        }
    }
}

fn check(build: &Compilation) {
    let diagnostics = match &build.outcome {
        Ok(output) => {
            if let Some(map) = &output.source_map {
                let json = map.to_json(&JsonOptions::default());
                assert!(!json.contains("sourcesContent"), "source content in a default map");
            }
            &output.warnings
        }
        Err(diagnostics) => diagnostics,
    };
    for diagnostic in diagnostics {
        let spans = diagnostic
            .primary()
            .into_iter()
            .chain(diagnostic.labels().iter().map(|label| label.span));
        for span in spans {
            assert!(build.sources.location(span).is_some(), "unresolvable span {span:?}");
        }
        assert!(
            !diagnostic.message().starts_with("internal compiler error"),
            "broken evaluator invariant: {}",
            diagnostic.message()
        );
        let rendered = diagnostic.render(&build.sources);
        assert!(
            !rendered.chars().any(|c| c.is_control() && c != '\n'),
            "control character in {rendered:?}"
        );
    }
}

fuzz_target!(|data: &[u8]| {
    let (flags, rest) = data.split_first().map_or((0, &[][..]), |(f, r)| (*f, r));
    let parts: Vec<&[u8]> = rest.splitn(NAMES.len(), |&b| b == 0xFF).collect();
    let Ok(entry) = ModuleId::new(NAMES[0]) else {
        return;
    };
    let options = options(flags);

    let first = compile(&entry, &mut resolver(&parts), &options);
    check(&first);
    let second = compile(&entry, &mut resolver(&parts), &options);
    assert_eq!(summary(&first), summary(&second), "nondeterministic build");

    let mut cache = ModuleCache::new();
    let _ = compile_with_cache(&entry, &mut resolver(&parts), &options, &mut cache);
    let cached = compile_with_cache(&entry, &mut resolver(&parts), &options, &mut cache);
    assert_eq!(summary(&first), summary(&cached), "cached rebuild differs");
});
