//! Module resolution interface (LANGUAGE.md §14).
//!
//! The compiler core never touches the filesystem: callers inject a
//! [`ModuleResolver`]. This module also provides the lexical rules every
//! resolver shares — load-path validation and root-relative path joining —
//! and an in-memory resolver for tests and embedding.

use std::collections::BTreeMap;
use std::fmt;

/// The identity of a module: a normalized, root-relative path using `/`
/// separators, such as `components/button.weave`. Two loads that resolve to
/// the same `ModuleId` are the same module.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ModuleId(String);

impl ModuleId {
    /// Validates a root-relative module path: non-empty `/`-separated
    /// segments, none of them empty, `.` or `..`, no backslashes, and a
    /// `.weave` extension.
    pub fn new(path: impl Into<String>) -> Result<Self, ResolveError> {
        let path = path.into();
        let valid = path.ends_with(".weave")
            && !path.contains('\\')
            && path
                .split('/')
                .all(|segment| !matches!(segment, "" | "." | ".."));
        if valid {
            Ok(ModuleId(path))
        } else {
            Err(ResolveError::InvalidPath(format!(
                "`{path}` is not a normalized root-relative module path"
            )))
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The directory part, without a trailing `/` (empty at the root).
    fn parent(&self) -> &str {
        self.0.rsplit_once('/').map_or("", |(dir, _)| dir)
    }
}

impl fmt::Display for ModuleId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Why a module could not be resolved or loaded. Each variant maps to one
/// diagnostic code.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ResolveError {
    /// E7004: the specifier breaks the load-path rules.
    InvalidPath(String),
    /// E7006: the path leaves the configured root.
    OutsideRoot(String),
    /// E7001: no module exists at the resolved path.
    NotFound(String),
    /// E7001: the module exists but could not be read.
    Unreadable(String),
}

impl fmt::Display for ResolveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ResolveError::InvalidPath(m)
            | ResolveError::OutsideRoot(m)
            | ResolveError::NotFound(m)
            | ResolveError::Unreadable(m) => f.write_str(m),
        }
    }
}

impl std::error::Error for ResolveError {}

/// Source text loaded for a module.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LoadedModule {
    /// Name shown in diagnostics. Resolvers should not expose absolute
    /// personal paths here.
    pub display_name: String,
    /// Raw bytes; the compiler validates UTF-8 itself.
    pub bytes: Vec<u8>,
}

/// Finds and reads modules on behalf of the compiler.
///
/// Implementations must reject paths that leave their root, including
/// through symbolic links, and must not perform network access.
pub trait ModuleResolver {
    /// Resolves `specifier` from a `load` statement in `importer`.
    fn resolve(&mut self, importer: &ModuleId, specifier: &str) -> Result<ModuleId, ResolveError>;

    /// Reads a resolved module. `max_bytes` is the per-file limit; a resolver
    /// may stop reading once it is exceeded and return the bytes read so far
    /// (more than `max_bytes`), which the compiler reports as E8001.
    fn load(&mut self, id: &ModuleId, max_bytes: u64) -> Result<LoadedModule, ResolveError>;
}

/// Checks a `load` path against LANGUAGE.md §14: it must start with `./` or
/// `../`, use `/` separators and end in `.weave`. Absolute paths, URLs, bare
/// specifiers and backslashes are rejected (E7004).
pub fn validate_load_path(specifier: &str) -> Result<(), ResolveError> {
    let invalid = |reason: &str| {
        Err(ResolveError::InvalidPath(format!(
            "invalid load path `{specifier}`: {reason}"
        )))
    };
    if specifier.contains('\\') {
        return invalid("use `/` as the separator, not `\\`");
    }
    if !(specifier.starts_with("./") || specifier.starts_with("../")) {
        return invalid("paths must start with `./` or `../`");
    }
    if !specifier.ends_with(".weave") {
        return invalid("paths must end in `.weave`");
    }
    if specifier.split('/').skip(1).any(str::is_empty) {
        return invalid("empty path segment");
    }
    if specifier
        .rsplit('/')
        .next()
        .is_some_and(|name| matches!(name, ".weave" | "..weave" | ".." | "."))
    {
        return invalid("missing file name");
    }
    Ok(())
}

/// Validates `specifier` and joins it to the importer's directory, folding
/// `.` and `..` lexically. A path that climbs above the root is E7006.
///
/// Lexical joining does not account for symbolic links; filesystem
/// resolvers must additionally check the canonical path.
pub fn join_load_path(importer: &ModuleId, specifier: &str) -> Result<ModuleId, ResolveError> {
    validate_load_path(specifier)?;
    let mut segments: Vec<&str> = importer
        .parent()
        .split('/')
        .filter(|s| !s.is_empty())
        .collect();
    for segment in specifier.split('/') {
        match segment {
            "." => {}
            ".." => {
                if segments.pop().is_none() {
                    return Err(ResolveError::OutsideRoot(format!(
                        "`{specifier}` from `{importer}` leaves the module root"
                    )));
                }
            }
            other => segments.push(other),
        }
    }
    ModuleId::new(segments.join("/"))
}

/// A resolver over an in-memory map of root-relative paths to contents.
#[derive(Clone, Debug, Default)]
pub struct MemoryResolver {
    modules: BTreeMap<ModuleId, Vec<u8>>,
}

impl MemoryResolver {
    pub fn new() -> Self {
        MemoryResolver::default()
    }

    pub fn insert(&mut self, id: ModuleId, bytes: impl Into<Vec<u8>>) {
        self.modules.insert(id, bytes.into());
    }
}

impl ModuleResolver for MemoryResolver {
    fn resolve(&mut self, importer: &ModuleId, specifier: &str) -> Result<ModuleId, ResolveError> {
        let id = join_load_path(importer, specifier)?;
        if self.modules.contains_key(&id) {
            Ok(id)
        } else {
            Err(ResolveError::NotFound(format!("module `{id}` not found")))
        }
    }

    fn load(&mut self, id: &ModuleId, _max_bytes: u64) -> Result<LoadedModule, ResolveError> {
        let bytes = self
            .modules
            .get(id)
            .ok_or_else(|| ResolveError::NotFound(format!("module `{id}` not found")))?;
        Ok(LoadedModule {
            display_name: id.to_string(),
            bytes: bytes.clone(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(path: &str) -> ModuleId {
        ModuleId::new(path).unwrap()
    }

    #[test]
    fn accepts_relative_weave_paths() {
        for ok in [
            "./a.weave",
            "../a.weave",
            "./dir/a.weave",
            "../../x/y.weave",
            "./é/ü.weave",
            "./a/./b.weave",
        ] {
            assert_eq!(validate_load_path(ok), Ok(()), "{ok}");
        }
    }

    #[test]
    fn rejects_paths_breaking_the_load_rules() {
        for bad in [
            "a.weave",
            "/abs/a.weave",
            "C:/a.weave",
            "https://example.com/a.weave",
            "//cdn/a.weave",
            ".\\a.weave",
            "./dir\\a.weave",
            "./a.css",
            "./a",
            "./a.weave/",
            ".//a.weave",
            "./.weave",
            "./",
            "",
            "pkg/a.weave",
        ] {
            assert!(
                matches!(validate_load_path(bad), Err(ResolveError::InvalidPath(_))),
                "{bad}"
            );
        }
    }

    #[test]
    fn joins_relative_to_the_importer_directory() {
        assert_eq!(
            join_load_path(&id("main.weave"), "./a.weave"),
            Ok(id("a.weave"))
        );
        assert_eq!(
            join_load_path(&id("ui/button.weave"), "../tokens/color.weave"),
            Ok(id("tokens/color.weave"))
        );
        assert_eq!(
            join_load_path(&id("a/b/c.weave"), "./../.././d.weave"),
            Ok(id("d.weave"))
        );
    }

    #[test]
    fn climbing_above_the_root_is_outside_root() {
        assert!(matches!(
            join_load_path(&id("main.weave"), "../a.weave"),
            Err(ResolveError::OutsideRoot(_))
        ));
        assert!(matches!(
            join_load_path(&id("a/b.weave"), "../../a.weave"),
            Err(ResolveError::OutsideRoot(_))
        ));
        // Going up and back down inside the root is fine.
        assert_eq!(
            join_load_path(&id("a/b.weave"), "../a/c.weave"),
            Ok(id("a/c.weave"))
        );
    }

    #[test]
    fn module_ids_must_be_normalized() {
        for bad in [
            "",
            "a",
            "./a.weave",
            "a/../b.weave",
            "/a.weave",
            "a//b.weave",
            "a\\b.weave",
        ] {
            assert!(ModuleId::new(bad).is_err(), "{bad}");
        }
        assert_eq!(id("a/b.weave").as_str(), "a/b.weave");
    }

    #[test]
    fn memory_resolver_resolves_and_loads() {
        let mut resolver = MemoryResolver::new();
        resolver.insert(id("main.weave"), "x");
        resolver.insert(id("lib/a.weave"), "y");
        let a = resolver
            .resolve(&id("main.weave"), "./lib/a.weave")
            .unwrap();
        assert_eq!(resolver.load(&a, 10).unwrap().bytes, b"y");
        assert!(matches!(
            resolver.resolve(&id("main.weave"), "./missing.weave"),
            Err(ResolveError::NotFound(_))
        ));
        assert!(matches!(
            resolver.load(&id("missing.weave"), 10),
            Err(ResolveError::NotFound(_))
        ));
    }
}
