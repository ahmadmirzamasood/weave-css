//! The compilation entry point.

use std::rc::Rc;

use crate::diagnostic::{Code, Diagnostic, Diagnostics};
use crate::ir::Node;
use crate::modules::{self, DependencyGraph, ModuleCache};
use crate::options::{CompileOptions, Limit};
use crate::resolve::{ModuleId, ModuleResolver, ResolveError};
use crate::source::{SourceManager, Span};
use crate::source_map::SourceMap;
use crate::{emit, eval};

/// A successful build.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompileOutput {
    /// UTF-8 CSS with LF line breaks (LANGUAGE.md §15).
    pub css: String,
    /// The source map, when [`CompileOptions::source_map`] is set.
    pub source_map: Option<SourceMap>,
    /// Every module that contributed to the build, entry first, then in
    /// discovery order.
    pub dependencies: Vec<ModuleId>,
    pub warnings: Vec<Diagnostic>,
}

/// The result of [`compile`], with the sources needed to render diagnostics.
#[derive(Debug)]
pub struct Compilation {
    pub sources: SourceManager,
    /// The modules reached and their `load` edges. After a failure it holds
    /// what was discovered before the build stopped.
    pub graph: DependencyGraph,
    /// On failure: every reported diagnostic (errors and warnings), up to the
    /// diagnostic limit.
    pub outcome: Result<CompileOutput, Vec<Diagnostic>>,
}

/// Compiles the module `entry`, reading modules through `resolver`.
///
/// This build compiles the Weave language of LANGUAGE.md: CSS with `rule`
/// blocks, bindings, expressions, `#{…}` interpolation, functions, control
/// flow, recipes, nesting, modules and the built-ins of docs/BUILTINS.md.
/// The `color` built-ins are reported as E5001 instead of being ignored.
pub fn compile(
    entry: &ModuleId,
    resolver: &mut dyn ModuleResolver,
    options: &CompileOptions,
) -> Compilation {
    compile_with_cache(entry, resolver, options, &mut ModuleCache::new())
}

/// [`compile`], reusing parsed modules from `cache` where their source text
/// is unchanged and storing the modules it parses (see [`ModuleCache`]).
pub fn compile_with_cache(
    entry: &ModuleId,
    resolver: &mut dyn ModuleResolver,
    options: &CompileOptions,
    cache: &mut ModuleCache,
) -> Compilation {
    compile_observed(entry, resolver, options, cache, &mut |_| {})
}

/// A point a build has reached, reported by [`compile_observed`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    /// Every module has been read and parsed (or taken from the cache).
    Loaded,
    /// Every module has been evaluated.
    Evaluated,
    /// The CSS (and source map, when requested) has been written.
    Emitted,
}

/// [`compile_with_cache`], calling `observer` as the build passes each
/// [`Phase`]. A failed build stops reporting at the phase that failed.
///
/// This exists for benchmarks and profiling: the observer may read a clock,
/// the compiler itself never does.
pub fn compile_observed(
    entry: &ModuleId,
    resolver: &mut dyn ModuleResolver,
    options: &CompileOptions,
    cache: &mut ModuleCache,
    observer: &mut dyn FnMut(Phase),
) -> Compilation {
    let mut sources = SourceManager::new();
    let limit = usize::try_from(options.limits.get(Limit::Diagnostics)).unwrap_or(usize::MAX);
    let mut diagnostics = Diagnostics::new(limit);

    let found = modules::discover(
        entry,
        resolver,
        options,
        &mut sources,
        &mut diagnostics,
        cache,
    );
    let graph = found.graph();
    if diagnostics.has_errors() {
        return failed(sources, graph, diagnostics);
    }
    observer(Phase::Loaded);

    // Evaluate dependencies first: unit `i` is `found.emission[i]`.
    let mut unit_of = vec![usize::MAX; found.modules.len()];
    for (unit, &module) in found.emission.iter().enumerate() {
        if let Some(slot) = unit_of.get_mut(module) {
            *slot = unit;
        }
    }
    let mut units = Vec::with_capacity(found.emission.len());
    for &index in &found.emission {
        let Some(module) = found.modules.get(index) else {
            continue;
        };
        let (Some(sheet), Some(file)) = (module.sheet.as_deref(), module.file) else {
            continue;
        };
        let imports = sheet
            .loads
            .iter()
            .zip(&module.targets)
            .filter_map(|(load, target)| {
                let unit = target.and_then(|t| unit_of.get(t).copied())?;
                Some((Rc::clone(&load.name), unit))
            })
            .collect();
        units.push(eval::Unit {
            items: &sheet.items,
            file,
            name: module.id.to_string(),
            exports: &sheet.exports,
            imports,
        });
    }
    let outputs = eval::evaluate(&units, &options.limits, &mut diagnostics);
    if diagnostics.has_errors() {
        return failed(sources, graph, diagnostics);
    }
    observer(Phase::Evaluated);

    let nodes = concatenate(outputs, &units, &mut diagnostics);
    if diagnostics.has_errors() {
        return failed(sources, graph, diagnostics);
    }
    match emit::emit(
        &nodes,
        options.limits.get(Limit::OutputBytes),
        options.source_map,
    ) {
        Ok((css, mappings)) => {
            let source_map = options
                .source_map
                .then(|| SourceMap::build(mappings, &sources));
            observer(Phase::Emitted);
            Compilation {
                outcome: Ok(CompileOutput {
                    css,
                    source_map,
                    dependencies: graph.modules().to_vec(),
                    warnings: diagnostics.into_vec(),
                }),
                sources,
                graph,
            }
        }
        Err(diagnostic) => {
            diagnostics.push(diagnostic);
            failed(sources, graph, diagnostics)
        }
    }
}

fn failed(sources: SourceManager, graph: DependencyGraph, diagnostics: Diagnostics) -> Compilation {
    Compilation {
        sources,
        graph,
        outcome: Err(diagnostics.into_vec()),
    }
}

/// How a top-level node takes part in `@import`/`@namespace` ordering
/// (CSS-COMPATIBILITY.md §5.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Order {
    Import,
    Namespace,
    /// `@charset`, `@layer` statements and comments.
    Neutral,
    Other,
}

fn order_of(node: &Node) -> Order {
    match node {
        Node::AtRule {
            name, body: None, ..
        } => match name.to_ascii_lowercase().as_str() {
            "@import" => Order::Import,
            "@namespace" => Order::Namespace,
            "@charset" | "@layer" => Order::Neutral,
            _ => Order::Other,
        },
        Node::Verbatim { text, .. } if is_comment(text) => Order::Neutral,
        _ => Order::Other,
    }
}

/// Whether `text` is exactly one CSS comment.
fn is_comment(text: &str) -> bool {
    text.len() >= 4
        && text.starts_with("/*")
        && text.find("*/").is_some_and(|end| end + 2 == text.len())
}

fn origin_span(node: &Node) -> Option<Span> {
    match node {
        Node::Rule { origin, .. }
        | Node::Declaration { origin, .. }
        | Node::AtRule { origin, .. }
        | Node::Keyframe { origin, .. }
        | Node::Verbatim { origin, .. } => origin.as_ref().map(|o| o.span),
    }
}

/// Joins the modules' CSS in emission order. The entry module's leading
/// `@charset` is moved to the very start (CSS-COMPATIBILITY.md §5.2); it is
/// the only item that moves. A native `@import` or `@namespace` that would
/// follow CSS another module emitted is E7010: CSS is never reordered to
/// satisfy CSS ordering rules.
fn concatenate(
    outputs: Vec<Vec<Node>>,
    units: &[eval::Unit<'_>],
    diags: &mut Diagnostics,
) -> Vec<Node> {
    let count = outputs.len();
    let mut nodes = Vec::new();
    let mut charset = None;
    // The first node (and its module) after which `@import`, respectively
    // `@namespace`, is no longer allowed.
    let mut import_blocker: Option<(Option<Span>, &str)> = None;
    let mut namespace_blocker: Option<(Option<Span>, &str)> = None;
    for (index, (output, unit)) in outputs.into_iter().zip(units).enumerate() {
        let entry = index + 1 == count;
        for (position, node) in output.into_iter().enumerate() {
            let order = order_of(&node);
            if entry && position == 0 {
                if let Node::AtRule { name, .. } = &node {
                    if name.eq_ignore_ascii_case("@charset") {
                        charset = Some(node);
                        continue;
                    }
                }
            }
            let blocker = match order {
                Order::Import => import_blocker,
                Order::Namespace => namespace_blocker,
                Order::Neutral | Order::Other => None,
            };
            if let Some((earlier, module)) = blocker {
                let name = match &node {
                    Node::AtRule { name, .. } => name.as_str(),
                    _ => "@import",
                };
                let mut diagnostic = Diagnostic::error(
                    Code::E7010,
                    format!("`{name}` would follow CSS emitted by module `{module}`; Weave does not reorder CSS"),
                )
                .with_note("a module's CSS is emitted after the CSS of the modules it loads, so this statement would be invalid in the output")
                .with_note(format!("move `{name}` into the first module that is loaded, or into a module emitted before `{module}`"));
                if let Some(span) = origin_span(&node) {
                    diagnostic = diagnostic.with_primary(span);
                }
                if let Some(span) = earlier {
                    diagnostic =
                        diagnostic.with_label(span, format!("emitted earlier, by `{module}`"));
                }
                diags.push(diagnostic);
            }
            match order {
                Order::Other => {
                    let here = (origin_span(&node), unit.name.as_str());
                    import_blocker.get_or_insert(here);
                    namespace_blocker.get_or_insert(here);
                }
                Order::Namespace => {
                    import_blocker.get_or_insert((origin_span(&node), unit.name.as_str()));
                }
                Order::Import | Order::Neutral => {}
            }
            nodes.push(node);
        }
    }
    if let Some(charset) = charset {
        nodes.insert(0, charset);
    }
    nodes
}

/// Maps a resolver failure to its diagnostic code (LANGUAGE.md §16).
pub fn resolve_diagnostic(err: &ResolveError) -> Diagnostic {
    let code = match err {
        ResolveError::InvalidPath(_) => Code::E7004,
        ResolveError::OutsideRoot(_) => Code::E7006,
        ResolveError::NotFound(_) | ResolveError::Unreadable(_) => Code::E7001,
    };
    Diagnostic::error(code, err.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(bytes: &[u8], options: &CompileOptions) -> Compilation {
        let mut resolver = crate::MemoryResolver::new();
        let entry = ModuleId::new("main.weave").unwrap();
        resolver.insert(entry.clone(), bytes);
        compile(&entry, &mut resolver, options)
    }

    fn codes(compilation: &Compilation) -> Vec<Code> {
        match &compilation.outcome {
            Ok(_) => Vec::new(),
            Err(diags) => diags.iter().map(Diagnostic::code).collect(),
        }
    }

    fn css(compilation: &Compilation) -> &str {
        match &compilation.outcome {
            Ok(output) => &output.css,
            Err(diags) => panic!("compilation failed: {diags:?}"),
        }
    }

    #[test]
    fn plain_css_compiles() {
        let result = run(
            b"rule a { color: red; }
",
            &CompileOptions::default(),
        );
        assert_eq!(
            css(&result),
            "a {
  color: red;
}
"
        );
        let Ok(output) = &result.outcome else {
            unreachable!()
        };
        assert_eq!(output.dependencies, [ModuleId::new("main.weave").unwrap()]);
        assert!(output.warnings.is_empty());
        assert_eq!(output.source_map, None);
    }

    #[test]
    fn empty_source_produces_zero_bytes() {
        assert_eq!(css(&run(b"", &CompileOptions::default())), "");
        assert_eq!(
            css(&run(b"\xef\xbb\xbf /* x */ ;", &CompileOptions::default())),
            ""
        );
    }

    #[test]
    fn missing_dependency_is_reported_at_the_load() {
        let result = run(
            b"load \"./a.weave\" as a;
rule a { color: red; }
",
            &CompileOptions::default(),
        );
        assert_eq!(codes(&result), [Code::E7001]);
    }

    #[test]
    fn bindings_are_evaluated() {
        let result = run(
            b"bind x = 2px * 3;
rule a { width: #{x}; }
",
            &CompileOptions::default(),
        );
        assert_eq!(css(&result), "a {\n  width: 6px;\n}\n");
    }

    #[test]
    fn evaluation_errors_stop_output() {
        let result = run(
            b"rule a { width: #{1px + 1em}; }",
            &CompileOptions::default(),
        );
        assert_eq!(codes(&result), [Code::E3001]);
    }

    #[test]
    fn source_text_errors_come_before_unsupported() {
        assert_eq!(
            codes(&run(b"a\xff", &CompileOptions::default())),
            [Code::E0001]
        );
        assert_eq!(
            codes(&run(b"a\0", &CompileOptions::default())),
            [Code::E0006]
        );
    }

    #[test]
    fn file_size_limit_is_inclusive() {
        let mut options = CompileOptions::default();
        options.limits.set(Limit::FileBytes, 4).unwrap();
        assert_eq!(codes(&run(b"/**/", &options)), []);
        assert_eq!(codes(&run(b"/***/", &options)), [Code::E8001]);
    }

    #[test]
    fn output_limit_applies() {
        let mut options = CompileOptions::default();
        options.limits.set(Limit::OutputBytes, 20).unwrap();
        assert_eq!(css(&run(b"rule a { color: red; }", &options)).len(), 20);
        options.limits.set(Limit::OutputBytes, 19).unwrap();
        assert_eq!(
            codes(&run(b"rule a { color: red; }", &options)),
            [Code::E8006]
        );
    }

    #[test]
    fn total_input_limit_applies() {
        let mut options = CompileOptions::default();
        options.limits.set(Limit::TotalInputBytes, 3).unwrap();
        assert_eq!(codes(&run(b"abcd", &options)), [Code::E8001]);
    }

    #[test]
    fn observer_sees_each_phase_once_in_order() {
        let observe = |bytes: &[u8]| {
            let mut resolver = crate::MemoryResolver::new();
            let entry = ModuleId::new("main.weave").unwrap();
            resolver.insert(entry.clone(), bytes);
            let mut phases = Vec::new();
            let result = compile_observed(
                &entry,
                &mut resolver,
                &CompileOptions::default(),
                &mut ModuleCache::new(),
                &mut |phase| phases.push(phase),
            );
            (phases, result.outcome.is_ok())
        };
        assert_eq!(
            observe(b"rule a { color: red; }"),
            (vec![Phase::Loaded, Phase::Evaluated, Phase::Emitted], true)
        );
        assert_eq!(
            observe(b"rule a { width: #{1px + 1em}; }"),
            (vec![Phase::Loaded], false)
        );
        assert_eq!(observe(b"rule a { color: red;"), (vec![], false));
    }

    #[test]
    fn missing_entry_is_e7001() {
        let mut resolver = crate::MemoryResolver::new();
        let entry = ModuleId::new("main.weave").unwrap();
        let result = compile(&entry, &mut resolver, &CompileOptions::default());
        assert_eq!(codes(&result), [Code::E7001]);
    }

    #[test]
    fn resolver_errors_map_to_their_codes() {
        let cases = [
            (ResolveError::InvalidPath(String::new()), Code::E7004),
            (ResolveError::OutsideRoot(String::new()), Code::E7006),
            (ResolveError::NotFound(String::new()), Code::E7001),
            (ResolveError::Unreadable(String::new()), Code::E7001),
        ];
        for (err, code) in cases {
            assert_eq!(resolve_diagnostic(&err).code(), code);
        }
    }
}
