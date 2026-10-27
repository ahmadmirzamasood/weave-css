//! The compilation entry point.

use crate::diagnostic::{Code, Diagnostic, Diagnostics};
use crate::options::{CompileOptions, Limit};
use crate::resolve::{ModuleId, ModuleResolver, ResolveError};
use crate::source::SourceManager;

/// A successful build.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompileOutput {
    /// UTF-8 CSS with LF line breaks (LANGUAGE.md §15).
    pub css: String,
    /// An ECMA-426 source map, when requested. Not produced yet.
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
/// This build reads and validates the entry module (resolution, size limits,
/// UTF-8 and U+0000 checks) and then reports E5001: the Weave language
/// itself is not implemented yet, so no CSS is ever produced.
pub fn compile(
    entry: &ModuleId,
    resolver: &mut dyn ModuleResolver,
    options: &CompileOptions,
) -> Compilation {
    let mut sources = SourceManager::new();
    let limit = usize::try_from(options.limits.get(Limit::Diagnostics)).unwrap_or(usize::MAX);
    let mut diagnostics = Diagnostics::new(limit);

    if let Err(diagnostic) = load_module(entry, resolver, options, &mut sources) {
        diagnostics.push(diagnostic);
        return failed(sources, diagnostics);
    }

    diagnostics.push(
        Diagnostic::error(
            Code::E5001,
            "compiling Weave source is not implemented in this build",
        )
        .with_note(format!(
            "`{entry}` was read and passed source-text validation; no CSS was produced"
        ))
        .with_note(format!(
            "weave-compiler {} provides only the compiler foundation",
            crate::VERSION
        )),
    );
    failed(sources, diagnostics)
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
) -> Result<(), Diagnostic> {
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

    sources.add(module.display_name, module.bytes).map(|_| ())
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

    #[test]
    fn valid_source_reports_unsupported_instead_of_output() {
        let result = run(b"rule a { color: red; }\n", &CompileOptions::default());
        assert_eq!(codes(&result), [Code::E5001]);
        let Err(diags) = &result.outcome else {
            panic!("compilation must not succeed");
        };
        assert!(diags[0].is_error());
        assert!(diags[0].message().contains("not implemented"));
        assert_eq!(result.sources.files().count(), 1);
    }

    #[test]
    fn empty_source_is_still_unsupported() {
        assert_eq!(codes(&run(b"", &CompileOptions::default())), [Code::E5001]);
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
        assert_eq!(codes(&run(b"abcd", &options)), [Code::E5001]);
        assert_eq!(codes(&run(b"abcde", &options)), [Code::E8001]);
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
