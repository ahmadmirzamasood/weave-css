//! Module loading (LANGUAGE.md §14): depth-first traversal of `load`
//! statements, canonical identity, cycle detection, the dependency graph
//! and the parsed-module cache.
//!
//! Traversal uses an explicit stack, so deep `load` chains cannot overflow
//! the Rust stack. Each module is read and parsed once per build, even when
//! several modules load it. The emission order is the post-order of the
//! traversal: a module follows every module it loads, at its first
//! encounter, and the entry module comes last.

use std::collections::HashMap;
use std::rc::Rc;

use crate::diagnostic::{Code, Diagnostic, Diagnostics};
use crate::options::{CompileOptions, Limit};
use crate::parser;
use crate::resolve::{ModuleId, ModuleResolver, ResolveError};
use crate::source::{FileId, SourceManager};
use crate::syntax::{Load, Stylesheet};

/// The modules of a build and the `load` edges between them.
///
/// On a failed build the graph holds what was discovered before the
/// failure, which is still useful for deciding what to watch.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DependencyGraph {
    /// Discovery order; the entry module first.
    modules: Vec<ModuleId>,
    /// For each module, the modules it loads, in `load` order, each once.
    edges: Vec<Vec<usize>>,
    /// Indices in emission order (dependencies first, entry last).
    emission: Vec<usize>,
}

impl DependencyGraph {
    /// Every module reached, in discovery order (the entry first).
    pub fn modules(&self) -> &[ModuleId] {
        &self.modules
    }

    pub fn contains(&self, id: &ModuleId) -> bool {
        self.position(id).is_some()
    }

    /// The modules `id` loads directly, in `load` order.
    pub fn dependencies(&self, id: &ModuleId) -> Vec<&ModuleId> {
        self.position(id)
            .and_then(|i| self.edges.get(i))
            .map(|targets| {
                targets
                    .iter()
                    .filter_map(|&t| self.modules.get(t))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// The modules that load `id` directly, in discovery order.
    pub fn dependents(&self, id: &ModuleId) -> Vec<&ModuleId> {
        let Some(target) = self.position(id) else {
            return Vec::new();
        };
        self.edges
            .iter()
            .zip(&self.modules)
            .filter(|(targets, _)| targets.contains(&target))
            .map(|(_, module)| module)
            .collect()
    }

    /// `id` and every module that loads it directly or indirectly, in
    /// discovery order: the modules whose output may change when `id`
    /// changes.
    pub fn affected_by(&self, id: &ModuleId) -> Vec<&ModuleId> {
        let Some(start) = self.position(id) else {
            return Vec::new();
        };
        // Reverse edges once, so the walk is linear in the graph's size.
        let mut importers: Vec<Vec<usize>> = vec![Vec::new(); self.modules.len()];
        for (importer, targets) in self.edges.iter().enumerate() {
            for &target in targets {
                if let Some(list) = importers.get_mut(target) {
                    list.push(importer);
                }
            }
        }
        let mut affected = vec![false; self.modules.len()];
        let mut work = vec![start];
        while let Some(current) = work.pop() {
            match affected.get_mut(current) {
                Some(seen) if !*seen => *seen = true,
                _ => continue,
            }
            if let Some(list) = importers.get(current) {
                work.extend(list);
            }
        }
        self.modules
            .iter()
            .zip(affected)
            .filter_map(|(module, hit)| hit.then_some(module))
            .collect()
    }

    /// Modules in CSS emission order: post-order, first encounter, the
    /// entry last (CSS-COMPATIBILITY.md §5.1).
    pub fn emission_order(&self) -> Vec<&ModuleId> {
        self.emission
            .iter()
            .filter_map(|&i| self.modules.get(i))
            .collect()
    }

    fn position(&self, id: &ModuleId) -> Option<usize> {
        self.modules.iter().position(|m| m == id)
    }
}

/// Hit and miss counts of a [`ModuleCache`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CacheStats {
    /// Modules whose parsed form was reused.
    pub hits: u64,
    /// Modules that were parsed.
    pub misses: u64,
}

/// Everything a parsed tree depends on. A tree is reused only when every
/// field matches the module as read in the new build.
#[derive(Debug, PartialEq, Eq)]
struct CacheKey {
    /// The compiler that parsed it ([`crate::VERSION`]).
    compiler: &'static str,
    /// The exact source text, byte-order mark included.
    text: String,
    /// Spans in the tree name this file.
    file: FileId,
    /// Only the entry module may start with `@charset`. (The entry is
    /// always file 0, so `file` changes with the role too.)
    entry: bool,
    /// The `nesting-depth` limit, the only limit the parser applies.
    max_depth: u32,
}

#[derive(Debug)]
struct CacheEntry {
    key: CacheKey,
    sheet: Rc<Stylesheet>,
}

/// Parsed modules kept between builds.
///
/// The cache only ever saves parsing. Every build still reads every module
/// through the resolver, and a parsed tree is reused only when everything
/// it was derived from is unchanged: the module identity, the compiler
/// version, the source text byte for byte, the module's role (entry or
/// not), the `nesting-depth` limit (the only limit the parser applies) and
/// the file's position in the new build's [`SourceManager`] (spans in the
/// tree name it). A changed module is therefore always parsed again,
/// whatever the caller does. The other limits are checked on every build
/// before the cache is consulted, and source maps are built from each
/// build's own sources, so file names and source-map options never come
/// from the cache. Evaluation is never cached: imported values, and so
/// every effect of a changed dependency, are recomputed in every build.
///
/// Trees whose parse reported an error are never stored. After a build has
/// discovered its modules, entries for modules it did not reach (deleted,
/// renamed or no longer loaded) are dropped, so the cache holds at most the
/// modules of the latest build.
///
/// [`ModuleCache::invalidate`] and [`ModuleCache::clear`] drop entries
/// explicitly; together with [`DependencyGraph::affected_by`] they are hooks
/// for callers that track changes themselves. Correctness never depends on
/// calling them.
#[derive(Debug, Default)]
pub struct ModuleCache {
    entries: HashMap<ModuleId, CacheEntry>,
    stats: CacheStats,
}

impl ModuleCache {
    pub fn new() -> Self {
        ModuleCache::default()
    }

    /// Drops the parsed form of `id`. Returns whether it was cached.
    pub fn invalidate(&mut self, id: &ModuleId) -> bool {
        self.entries.remove(id).is_some()
    }

    /// Drops every entry. Statistics are kept.
    pub fn clear(&mut self) {
        self.entries.clear();
    }

    pub fn contains(&self, id: &ModuleId) -> bool {
        self.entries.contains_key(id)
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn stats(&self) -> CacheStats {
        self.stats
    }
}

/// One module of the build.
#[derive(Debug)]
pub(crate) struct Module {
    pub id: ModuleId,
    pub file: Option<FileId>,
    /// `None` when the module could not be read.
    pub sheet: Option<Rc<Stylesheet>>,
    /// The module each `load` statement refers to, when it resolved.
    pub targets: Vec<Option<usize>>,
    /// Whether traversal has finished the module.
    done: bool,
}

/// The result of traversal.
#[derive(Debug)]
pub(crate) struct Discovered {
    /// In discovery order; the entry module first.
    pub modules: Vec<Module>,
    /// Indices into `modules` in emission order.
    pub emission: Vec<usize>,
}

impl Discovered {
    pub fn graph(&self) -> DependencyGraph {
        let edges = self
            .modules
            .iter()
            .map(|module| {
                let mut targets: Vec<usize> = Vec::new();
                let mut seen = vec![false; self.modules.len()];
                for &t in module.targets.iter().flatten() {
                    if let Some(slot @ false) = seen.get_mut(t) {
                        *slot = true;
                        targets.push(t);
                    }
                }
                targets
            })
            .collect();
        DependencyGraph {
            modules: self.modules.iter().map(|m| m.id.clone()).collect(),
            edges,
            emission: self.emission.clone(),
        }
    }
}

struct Loader<'a> {
    resolver: &'a mut dyn ModuleResolver,
    options: &'a CompileOptions,
    sources: &'a mut SourceManager,
    diags: &'a mut Diagnostics,
    cache: &'a mut ModuleCache,
    max_depth: u32,
}

/// Reads and parses `entry` and every module it loads, transitively.
/// Problems are reported to `diags`; traversal continues past them so that
/// independent errors in other modules are reported too.
pub(crate) fn discover(
    entry: &ModuleId,
    resolver: &mut dyn ModuleResolver,
    options: &CompileOptions,
    sources: &mut SourceManager,
    diags: &mut Diagnostics,
    cache: &mut ModuleCache,
) -> Discovered {
    let mut loader = Loader {
        resolver,
        options,
        sources,
        diags,
        cache,
        max_depth: u32::try_from(options.limits.get(Limit::NestingDepth)).unwrap_or(u32::MAX),
    };
    let mut modules = vec![loader.open(entry.clone(), None)];
    let mut index: HashMap<ModuleId, usize> = HashMap::from([(entry.clone(), 0)]);
    let mut emission = Vec::new();
    // (module, next load to follow)
    let mut stack: Vec<(usize, usize)> = vec![(0, 0)];
    while let Some(&(current, next)) = stack.last() {
        let Some(module) = modules.get(current) else {
            break;
        };
        let sheet = module.sheet.clone();
        let Some(load) = sheet.as_deref().and_then(|s| s.loads.get(next)) else {
            stack.pop();
            if let Some(module) = modules.get_mut(current) {
                module.done = true;
            }
            emission.push(current);
            continue;
        };
        if let Some(top) = stack.last_mut() {
            top.1 += 1;
        }
        if loader.diags.is_full() {
            break;
        }
        let importer = module.id.clone();
        let target = match loader.resolver.resolve(&importer, &load.path) {
            Err(err) => {
                loader.diags.push(load_error(&err, &importer, load));
                None
            }
            Ok(id) => match index.get(&id) {
                Some(&target) if modules.get(target).is_some_and(|m| !m.done) => {
                    let chain = stack
                        .iter()
                        .filter_map(|&(m, _)| modules.get(m))
                        .map(|m| m.id.to_string())
                        .chain(std::iter::once(id.to_string()))
                        .collect::<Vec<_>>()
                        .join(" -> ");
                    loader.diags.push(
                        Diagnostic::error(Code::E7002, format!("load cycle: {chain}"))
                            .with_primary(load.span)
                            .with_label(load.path_span, format!("`{id}` is still being loaded"))
                            .with_note("modules cannot load each other in a cycle; move the shared definitions into a module that both can load"),
                    );
                    None
                }
                Some(&target) => Some(target),
                None => {
                    if modules.len() as u64 >= options.limits.get(Limit::Modules) {
                        loader.diags.push(
                            Diagnostic::error(
                                Code::E8008,
                                format!(
                                    "the build exceeds the module limit of {}",
                                    options.limits.get(Limit::Modules)
                                ),
                            )
                            .with_primary(load.path_span)
                            .with_note(format!("raise it with the `{}` limit", Limit::Modules)),
                        );
                        None
                    } else {
                        let target = modules.len();
                        modules.push(loader.open(id.clone(), Some(load)));
                        index.insert(id, target);
                        stack.push((target, 0));
                        Some(target)
                    }
                }
            },
        };
        if let Some(module) = modules.get_mut(current) {
            module.targets.push(target);
        }
    }
    // Drop entries for modules this build no longer reaches.
    loader.cache.entries.retain(|id, _| index.contains_key(id));
    Discovered { modules, emission }
}

/// A resolver failure, reported at the path of the `load` statement.
fn load_error(err: &ResolveError, importer: &ModuleId, load: &Load) -> Diagnostic {
    let diagnostic = crate::compile::resolve_diagnostic(err)
        .with_primary(load.path_span)
        .with_note(format!("loaded from `{importer}`"));
    match err {
        ResolveError::InvalidPath(_) => {
            diagnostic.with_note("load paths start with `./` or `../`, use `/` and end in `.weave`")
        }
        ResolveError::OutsideRoot(_) => diagnostic.with_note(
            "modules must stay inside the module root, also after following symbolic links",
        ),
        ResolveError::NotFound(_) | ResolveError::Unreadable(_) => diagnostic,
    }
}

impl Loader<'_> {
    /// Reads and parses one module; `from` is the `load` statement that
    /// reached it (none for the entry).
    fn open(&mut self, id: ModuleId, from: Option<&Load>) -> Module {
        let entry = from.is_none();
        let at = |d: Diagnostic| match from {
            Some(load) if d.primary().is_none() => d.with_primary(load.path_span),
            _ => d,
        };
        let mut module = Module {
            id,
            file: None,
            sheet: None,
            targets: Vec::new(),
            done: false,
        };
        let file = match self.read(&module.id) {
            Ok(file) => file,
            Err(diagnostic) => {
                self.diags.push(at(diagnostic));
                self.cache.invalidate(&module.id);
                return module;
            }
        };
        module.file = Some(file);
        let Some(source) = self.sources.get(file) else {
            return module;
        };
        if let Some(cached) = self.cache.entries.get(&module.id) {
            let key = &cached.key;
            if key.compiler == crate::VERSION
                && key.file == file
                && key.entry == entry
                && key.max_depth == self.max_depth
                && key.text == source.text()
            {
                self.cache.stats.hits += 1;
                module.sheet = Some(Rc::clone(&cached.sheet));
                return module;
            }
        }
        self.cache.stats.misses += 1;
        // A parse that starts with the diagnostic list full could stop
        // early without recording an error, so its tree is never stored.
        let complete = !self.diags.is_full();
        let errors = error_count(self.diags);
        let sheet = Rc::new(parser::parse(
            source.text(),
            source.content_start() as usize,
            file,
            entry,
            self.max_depth,
            self.diags,
        ));
        if complete && error_count(self.diags) == errors && !self.diags.is_full() {
            self.cache.entries.insert(
                module.id.clone(),
                CacheEntry {
                    key: CacheKey {
                        compiler: crate::VERSION,
                        text: source.text().to_string(),
                        file,
                        entry,
                        max_depth: self.max_depth,
                    },
                    sheet: Rc::clone(&sheet),
                },
            );
        } else {
            self.cache.invalidate(&module.id);
        }
        module.sheet = Some(sheet);
        module
    }

    /// Reads a module through the resolver and registers it, enforcing the
    /// input limits.
    fn read(&mut self, id: &ModuleId) -> Result<FileId, Diagnostic> {
        let options = self.options;
        let file_limit = options.limits.get(Limit::FileBytes);
        let module = self
            .resolver
            .load(id, file_limit)
            .map_err(|err| crate::compile::resolve_diagnostic(&err))?;

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
        if self.sources.total_bytes().saturating_add(len) > total_limit {
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
        self.sources.add(module.display_name, module.bytes)
    }
}

fn error_count(diags: &Diagnostics) -> usize {
    diags.iter().filter(|d| d.is_error()).count()
}
