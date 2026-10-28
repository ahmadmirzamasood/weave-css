//! The compilation entry point.

use crate::diagnostic::{Code, Diagnostic, Diagnostics};
use crate::options::{CompileOptions, Limit};
use crate::resolve::{ModuleId, ModuleResolver, ResolveError};
use crate::source::{FileId, SourceManager};
use crate::{emit, eval, parser};

/// A successful build.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompileOutput {
    /// UTF-8 CSS with LF line breaks (LANGUAGE.md §15).
    pub css: String,
    /// An ECMA-426 source map, when requested. Not produced yet (phase 08).
    pub source_map: Option<String>,
    /// Every module that contributed to the build, entry first.
    pub dependencies: Vec<ModuleId>,
    pub warnings: Vec<Diagnostic>,
}

/// The result of [`compile`], with the sources needed to render diagnostics.
#[derive(Debug)]
pub struct Compilation {
    pub sources: SourceManager,
    /// On failure: every reported diagnostic (errors and warnings), up to the
    /// diagnostic limit.
    pub outcome: Result<CompileOutput, Vec<Diagnostic>>,
}

/// Compiles the module `entry`, reading modules through `resolver`.
///
/// This build compiles the plain-CSS subset of Weave (`rule` blocks,
/// declarations, known and unknown at-rules, `opaque` blocks and comments)
/// with `bind` statements, expressions, `#{…}` interpolation, functions,
/// control flow, recipes, nesting and the built-ins of docs/BUILTINS.md.
/// Modules and the `color` built-ins are reported as E5001 instead of being
/// ignored.
pub fn compile(
    entry: &ModuleId,
    resolver: &mut dyn ModuleResolver,
    options: &CompileOptions,
) -> Compilation {
    let mut sources = SourceManager::new();
    let limit = usize::try_from(options.limits.get(Limit::Diagnostics)).unwrap_or(usize::MAX);
    let mut diagnostics = Diagnostics::new(limit);

    let file = match load_module(entry, resolver, options, &mut sources) {
        Ok(file) => file,
        Err(diagnostic) => {
            diagnostics.push(diagnostic);
            return failed(sources, diagnostics);
        }
    };
    let Some(source) = sources.get(file) else {
        return failed(sources, diagnostics);
    };
    let max_depth = u32::try_from(options.limits.get(Limit::NestingDepth)).unwrap_or(u32::MAX);
    let sheet = parser::parse(
        source.text(),
        source.content_start() as usize,
        file,
        max_depth,
        &mut diagnostics,
    );
    if diagnostics.has_errors() {
        return failed(sources, diagnostics);
    }
    let nodes = eval::evaluate(&sheet.items, file, &options.limits, &mut diagnostics);
    if diagnostics.has_errors() {
        return failed(sources, diagnostics);
    }
    match emit::emit(&nodes, options.limits.get(Limit::OutputBytes)) {
        Ok(css) => Compilation {
            sources,
            outcome: Ok(CompileOutput {
                css,
                source_map: None,
                dependencies: vec![entry.clone()],
                warnings: diagnostics.into_vec(),
            }),
        },
        Err(diagnostic) => {
            diagnostics.push(diagnostic);
            failed(sources, diagnostics)
        }
    }
}

fn failed(sources: SourceManager, diagnostics: Diagnostics) -> Compilation {
    Compilation {
        sources,
        outcome: Err(diagnostics.into_vec()),
    }
}

fn load_module(
    id: &ModuleId,
    resolver: &mut dyn ModuleResolver,
    options: &CompileOptions,
    sources: &mut SourceManager,
) -> Result<FileId, Diagnostic> {
    let file_limit = options.limits.get(Limit::FileBytes);
    let module = resolver
        .load(id, file_limit)
        .map_err(|err| resolve_diagnostic(&err))?;

    let len = u64::try_from(module.bytes.len()).unwrap_or(u64::MAX);
    if len > file_limit {
        return Err(Diagnostic::error(
            Code::E8001,
            format!(
                "`{}` exceeds the per-file input limit of {file_limit} bytes",
                module.display_name
            ),
        )
        .with_note(format!("raise it with the `{}` limit", Limit::FileBytes)));
    }
    let total_limit = options.limits.get(Limit::TotalInputBytes);
    if sources.total_bytes().saturating_add(len) > total_limit {
        return Err(Diagnostic::error(
            Code::E8001,
            format!(
                "loading `{}` exceeds the total input limit of {total_limit} bytes",
                module.display_name
            ),
        )
        .with_note(format!(
            "raise it with the `{}` limit",
            Limit::TotalInputBytes
        )));
    }
    let module_limit = options.limits.get(Limit::Modules);
    if u64::try_from(sources.files().count()).unwrap_or(u64::MAX) >= module_limit {
        return Err(Diagnostic::error(
            Code::E8008,
            format!("the build exceeds the module limit of {module_limit}"),
        ));
    }

    sources.add(module.display_name, module.bytes)
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
    fn unimplemented_statements_are_reported() {
        let result = run(
            b"load \"./a.weave\" as a;
rule a { color: red; }
",
            &CompileOptions::default(),
        );
        assert_eq!(codes(&result), [Code::E5001]);
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
