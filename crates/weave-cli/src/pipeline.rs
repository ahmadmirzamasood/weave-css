//! One build: resolve the input, compile, check output paths, write outputs
//! and report. `build`, `check` and every `watch` rebuild share it.

use std::collections::BTreeSet;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use weave_compiler::diagnostic::escape_control;
use weave_compiler::{
    Compilation, CompileOptions, CompileOutput, JsonOptions, ModuleCache, ModuleId,
    compile_with_cache,
};

use crate::args::{DiagnosticFormat, Input, Options, SourceMapMode, Target, map_path};
use crate::fs_resolver::FsResolver;
use crate::output;
use crate::report::{Problem, Report};

/// What a build did, for the exit status and for `watch`.
#[derive(Debug, Default)]
pub struct Outcome {
    pub success: bool,
    /// Files whose creation, change or deletion can change the result.
    pub watched: BTreeSet<PathBuf>,
    /// Bytes of CSS written, after a successful build.
    pub css_bytes: usize,
    pub modules: usize,
    /// The diagnostics report for stderr, written by the caller (so that
    /// `watch` can record file state before announcing the build).
    pub report: String,
}

#[derive(Debug)]
pub struct Session {
    pub options: Options,
    pub command: &'static str,
    /// Standard input, read once before the first build.
    pub stdin: Option<Vec<u8>>,
    pub cache: ModuleCache,
}

impl Session {
    pub fn new(options: Options, command: &'static str, stdin: Option<Vec<u8>>) -> Session {
        Session {
            options,
            command,
            stdin,
            cache: ModuleCache::new(),
        }
    }

    /// Runs one build, writing outputs (CSS or dependencies go to stdout
    /// when selected); the diagnostics report is returned in the outcome.
    pub fn run(&mut self) -> Outcome {
        let mut problems = Vec::new();
        let mut outcome = Outcome::default();
        if let Input::File(path) = &self.options.input {
            outcome.watched.insert(absolute(path));
        }
        let Some((mut resolver, entry)) = self.resolver(&mut problems) else {
            outcome.report = self.report(None, &[], &problems, false, Vec::new());
            return outcome;
        };

        let mut compile_options = CompileOptions::default();
        compile_options.limits = self.options.limits.clone();
        compile_options.source_map = self.options.source_map.is_some();
        let compilation =
            compile_with_cache(&entry, &mut resolver, &compile_options, &mut self.cache);

        outcome.watched.extend(resolver.take_touched());
        let modules: Vec<ModuleId> = compilation.graph.modules().to_vec();
        for module in &modules {
            if let Some(path) = resolver.path(module) {
                outcome.watched.insert(path);
            }
        }
        outcome.modules = modules.len();
        let names: Vec<String> = modules.iter().map(ToString::to_string).collect();

        let Compilation {
            sources,
            outcome: result,
            ..
        } = compilation;
        match result {
            Err(diagnostics) => {
                outcome.report = self.report(Some(&sources), &diagnostics, &problems, false, names);
            }
            Ok(output) => {
                let written = self.write_outputs(&output, &resolver, &mut problems);
                outcome.success = written.is_some();
                outcome.css_bytes = written.unwrap_or(0);
                outcome.report = self.report(
                    Some(&sources),
                    &output.warnings,
                    &problems,
                    outcome.success,
                    names,
                );
            }
        }
        // Generated files are never inputs: drop them from the watched set
        // so that writing them cannot trigger a rebuild.
        for target in self.output_files() {
            outcome.watched.remove(&absolute(&target));
            if let Ok(id) = output::identity(&target) {
                outcome.watched.remove(&id);
            }
        }
        outcome
    }

    /// Builds the resolver and the entry identity.
    fn resolver(&self, problems: &mut Vec<Problem>) -> Option<(FsResolver, ModuleId)> {
        let shown = |path: &Path| escape_control(&path.to_string_lossy());
        let root = match (&self.options.root, &self.options.input) {
            (Some(root), _) => Some(root.clone()),
            (None, Input::Stdin) => Some(PathBuf::from(".")),
            (None, Input::File(_)) => None,
        };
        let resolver = match &root {
            Some(root) => match FsResolver::with_root(root) {
                Ok(resolver) => Some(resolver),
                Err(err) => {
                    problems.push(Problem::new(
                        None,
                        format!("cannot use `{}` as the module root: {}", shown(root), err),
                    ));
                    return None;
                }
            },
            None => None,
        };
        match (&self.options.input, resolver) {
            (Input::Stdin, Some(mut resolver)) => {
                let entry = resolver.set_stdin(self.stdin.clone().unwrap_or_default());
                Some((resolver, entry))
            }
            (Input::File(path), resolver) => {
                let found = match resolver {
                    Some(resolver) => resolver.entry_id(path).map(|id| (resolver, id)),
                    None => FsResolver::for_entry(path),
                };
                match found {
                    Ok(found) => Some(found),
                    Err(err) if err.kind() == io::ErrorKind::NotFound => {
                        problems.push(Problem::new(
                            Some("E7001"),
                            format!("input file `{}` not found", shown(path)),
                        ));
                        None
                    }
                    Err(err) => {
                        problems.push(Problem::new(
                            None,
                            format!("cannot use `{}` as input: {err}", shown(path)),
                        ));
                        None
                    }
                }
            }
            (Input::Stdin, None) => None,
        }
    }

    /// The output files this build may write (CSS, map, dependency list).
    fn output_files(&self) -> Vec<PathBuf> {
        let mut files = Vec::new();
        if let Some(Target::File(css)) = &self.options.output {
            if self.options.source_map == Some(SourceMapMode::File) {
                files.push(map_path(css));
            }
            files.push(css.clone());
        }
        if let Some(Target::File(deps)) = &self.options.deps {
            files.push(deps.clone());
        }
        files
    }

    /// Checks for collisions, then writes every selected output. Returns the
    /// CSS byte count, or `None` (with problems) when nothing was written.
    fn write_outputs(
        &self,
        output: &CompileOutput,
        resolver: &FsResolver,
        problems: &mut Vec<Problem>,
    ) -> Option<usize> {
        let shown = |path: &Path| escape_control(&path.to_string_lossy());
        let cwd = std::env::current_dir()
            .and_then(|dir| dir.canonicalize())
            .ok();

        // Output identities must differ from every input and from each other.
        let mut identities: Vec<(PathBuf, PathBuf)> = Vec::new();
        for file in self.output_files() {
            match output::identity(&file) {
                Ok(id) => identities.push((file, id)),
                Err(err) => {
                    problems.push(Problem::new(
                        None,
                        format!("cannot write `{}`: {err}", shown(&file)),
                    ));
                    return None;
                }
            }
        }
        for (i, (file, id)) in identities.iter().enumerate() {
            for module in &output.dependencies {
                let Some(path) = resolver.path(module) else {
                    continue;
                };
                let module_id = path.canonicalize().unwrap_or(path);
                if output::same_file(id, &module_id) {
                    problems.push(Problem::new(
                        None,
                        format!(
                            "output `{}` is the input module `{module}`; refusing to overwrite it",
                            shown(file)
                        ),
                    ));
                }
            }
            for (other, other_id) in identities.iter().skip(i + 1) {
                if output::same_file(id, other_id) {
                    problems.push(Problem::new(
                        None,
                        format!(
                            "outputs `{}` and `{}` are the same file",
                            shown(file),
                            shown(other)
                        ),
                    ));
                }
            }
        }
        if !problems.is_empty() {
            return None;
        }

        let mut css = output.css.clone();
        let mut map_file = None;
        if let (Some(mode), Some(map)) = (self.options.source_map, &output.source_map) {
            let css_identity = match &self.options.output {
                Some(Target::File(path)) => output::identity(path).ok(),
                _ => None,
            };
            let base = css_identity
                .as_deref()
                .and_then(Path::parent)
                .map(Path::to_path_buf)
                .or_else(|| cwd.clone());
            let paths: Vec<String> = map
                .sources()
                .iter()
                .map(|name| {
                    let path = ModuleId::new(name.as_str())
                        .ok()
                        .and_then(|id| resolver.path(&id));
                    match (&base, path) {
                        (Some(base), Some(path)) => {
                            output::relative(base, &path).unwrap_or_else(|| name.clone())
                        }
                        _ => name.clone(),
                    }
                })
                .collect();
            let file_name = match &self.options.output {
                Some(Target::File(path)) => {
                    path.file_name().map(|n| n.to_string_lossy().into_owned())
                }
                _ => None,
            };
            let json = map.to_json(&JsonOptions {
                file: file_name.as_deref(),
                sources: Some(&paths),
                sources_content: self.options.source_map_content,
            });
            let url = match mode {
                SourceMapMode::Inline => format!(
                    "data:application/json;charset=utf-8;base64,{}",
                    output::base64(json.as_bytes())
                ),
                SourceMapMode::File => {
                    let name = file_name.clone().unwrap_or_default();
                    output::url_path(&format!("{name}.map"))
                }
            };
            css.push_str(&format!("/*# sourceMappingURL={url} */\n"));
            if mode == SourceMapMode::File {
                if let Some(Target::File(path)) = &self.options.output {
                    map_file = Some((map_path(path), json));
                }
            }
        }

        let deps_text: String = output
            .dependencies
            .iter()
            .filter_map(|module| {
                let path = resolver.path(module)?;
                let shown = cwd
                    .as_deref()
                    .and_then(|cwd| output::relative(cwd, &path))
                    .unwrap_or_else(|| module.to_string());
                Some(format!("{shown}\n"))
            })
            .collect();

        // The map goes first so that a CSS file never names a missing map.
        if let Some((path, json)) = map_file {
            if let Err(err) = output::write_atomic(&path, json.as_bytes()) {
                problems.push(Problem::new(
                    None,
                    format!("cannot write `{}`: {err}", shown(&path)),
                ));
                return None;
            }
        }
        match &self.options.output {
            Some(Target::File(path)) => {
                if let Err(err) = output::write_atomic(path, css.as_bytes()) {
                    problems.push(Problem::new(
                        None,
                        format!("cannot write `{}`: {err}", shown(path)),
                    ));
                    return None;
                }
            }
            Some(Target::Stdout) => {
                if let Err(err) = write_stdout(css.as_bytes()) {
                    problems.push(Problem::new(None, format!("cannot write to stdout: {err}")));
                    return None;
                }
            }
            None => {}
        }
        match &self.options.deps {
            Some(Target::File(path)) => {
                if let Err(err) = output::write_atomic(path, deps_text.as_bytes()) {
                    problems.push(Problem::new(
                        None,
                        format!("cannot write `{}`: {err}", shown(path)),
                    ));
                    return None;
                }
            }
            Some(Target::Stdout) => {
                if let Err(err) = write_stdout(deps_text.as_bytes()) {
                    problems.push(Problem::new(None, format!("cannot write to stdout: {err}")));
                    return None;
                }
            }
            None => {}
        }
        Some(css.len())
    }

    fn report(
        &self,
        sources: Option<&weave_compiler::SourceManager>,
        diagnostics: &[weave_compiler::Diagnostic],
        problems: &[Problem],
        success: bool,
        dependencies: Vec<String>,
    ) -> String {
        let report = Report {
            command: self.command,
            success,
            output: self.options.output.as_ref().map(|target| match target {
                Target::Stdout => "-".to_string(),
                Target::File(path) => path.to_string_lossy().into_owned(),
            }),
            dependencies,
            sources,
            diagnostics,
            problems,
        };
        match self.options.diagnostics {
            DiagnosticFormat::Human => report.human(),
            DiagnosticFormat::Json => report.json(),
        }
    }
}

/// Writes a report to stderr.
pub fn write_stderr(text: &str) -> io::Result<()> {
    let mut stderr = io::stderr().lock();
    stderr.write_all(text.as_bytes())?;
    stderr.flush()
}

fn write_stdout(bytes: &[u8]) -> io::Result<()> {
    let mut stdout = io::stdout().lock();
    stdout.write_all(bytes)?;
    stdout.flush()
}

/// `path` made absolute against the current directory (not canonicalized,
/// so it also works for files that do not exist).
pub fn absolute(path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map(|dir| dir.join(path))
            .unwrap_or_else(|_| path.to_path_buf())
    }
}
