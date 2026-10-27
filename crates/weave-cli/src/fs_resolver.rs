//! Filesystem module resolver rooted at a directory (LANGUAGE.md §14).
//!
//! Module identities are canonical paths relative to the root, so symbolic
//! links that point outside the root are rejected (E7006). Checks use
//! ordinary paths: a concurrent filesystem change between the check and the
//! read is not defended against.

use std::fs::File;
use std::io::{self, Read};
use std::path::{Component, Path, PathBuf};

use weave_compiler::{LoadedModule, ModuleId, ModuleResolver, ResolveError, resolve};

#[derive(Debug)]
pub struct FsResolver {
    /// Canonical root directory.
    root: PathBuf,
}

impl FsResolver {
    /// Creates a resolver rooted at the entry file's directory and returns
    /// it with the entry's module identity.
    pub fn for_entry(entry: &Path) -> io::Result<(FsResolver, ModuleId)> {
        if entry.extension().is_none_or(|ext| ext != "weave") {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "input files must have the .weave extension",
            ));
        }
        let canonical = entry.canonicalize()?;
        if !canonical.is_file() {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "not a file"));
        }
        let root = canonical
            .parent()
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "no parent directory"))?
            .to_path_buf();
        let resolver = FsResolver { root };
        let id = resolver
            .module_id(&canonical)
            .map_err(|err| io::Error::new(io::ErrorKind::InvalidInput, err.to_string()))?;
        Ok((resolver, id))
    }

    fn path_of(&self, id: &ModuleId) -> PathBuf {
        let mut path = self.root.clone();
        path.extend(id.as_str().split('/'));
        path
    }

    /// The identity of a canonical path, or E7006 if it is outside the root.
    fn module_id(&self, canonical: &Path) -> Result<ModuleId, ResolveError> {
        let relative = canonical.strip_prefix(&self.root).map_err(|_| {
            ResolveError::OutsideRoot("the resolved path is outside the module root".to_string())
        })?;
        let mut segments = Vec::new();
        for component in relative.components() {
            match component {
                Component::Normal(part) => segments.push(part.to_str().ok_or_else(|| {
                    ResolveError::InvalidPath("module paths must be valid UTF-8".to_string())
                })?),
                _ => {
                    return Err(ResolveError::InvalidPath(
                        "unexpected component in a canonical path".to_string(),
                    ));
                }
            }
        }
        ModuleId::new(segments.join("/"))
    }

    /// Canonicalizes the path of `id`, following symbolic links.
    fn canonical(&self, id: &ModuleId) -> Result<PathBuf, ResolveError> {
        self.path_of(id)
            .canonicalize()
            .map_err(|err| io_error(id, &err))
    }
}

fn io_error(id: &ModuleId, err: &io::Error) -> ResolveError {
    if err.kind() == io::ErrorKind::NotFound {
        ResolveError::NotFound(format!("module `{id}` not found"))
    } else {
        ResolveError::Unreadable(format!("cannot read module `{id}`: {}", err.kind()))
    }
}

impl ModuleResolver for FsResolver {
    fn resolve(&mut self, importer: &ModuleId, specifier: &str) -> Result<ModuleId, ResolveError> {
        let lexical = resolve::join_load_path(importer, specifier)?;
        let canonical = self.canonical(&lexical)?;
        self.module_id(&canonical)
    }

    fn load(&mut self, id: &ModuleId, max_bytes: u64) -> Result<LoadedModule, ResolveError> {
        let canonical = self.canonical(id)?;
        let canonical_id = self.module_id(&canonical)?;
        let file = File::open(&canonical).map_err(|err| io_error(id, &err))?;
        let mut bytes = Vec::new();
        // Read one byte past the limit so the compiler can report E8001
        // without this resolver reading an unbounded file.
        file.take(max_bytes.saturating_add(1))
            .read_to_end(&mut bytes)
            .map_err(|err| io_error(id, &err))?;
        Ok(LoadedModule {
            display_name: canonical_id.to_string(),
            bytes,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::atomic::{AtomicU32, Ordering};

    fn temp_dir(name: &str) -> PathBuf {
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir =
            std::env::temp_dir().join(format!("weave-cli-test-{}-{n}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn resolves_and_loads_inside_the_root() {
        let dir = temp_dir("inside");
        fs::create_dir_all(dir.join("lib")).unwrap();
        fs::write(dir.join("main.weave"), "m").unwrap();
        fs::write(dir.join("lib/a.weave"), "abc").unwrap();
        let (mut resolver, entry) = FsResolver::for_entry(&dir.join("main.weave")).unwrap();
        assert_eq!(entry.as_str(), "main.weave");
        let a = resolver.resolve(&entry, "./lib/a.weave").unwrap();
        assert_eq!(a.as_str(), "lib/a.weave");
        let loaded = resolver.load(&a, 100).unwrap();
        assert_eq!(loaded.bytes, b"abc");
        assert_eq!(loaded.display_name, "lib/a.weave");
        // Reads stop one byte past the limit.
        assert_eq!(resolver.load(&a, 1).unwrap().bytes, b"ab");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_and_escaping_modules_are_rejected() {
        let dir = temp_dir("escape");
        fs::create_dir_all(dir.join("root")).unwrap();
        fs::write(dir.join("root/main.weave"), "m").unwrap();
        fs::write(dir.join("outside.weave"), "o").unwrap();
        let (mut resolver, entry) = FsResolver::for_entry(&dir.join("root/main.weave")).unwrap();
        assert!(matches!(
            resolver.resolve(&entry, "./missing.weave"),
            Err(ResolveError::NotFound(_))
        ));
        assert!(matches!(
            resolver.resolve(&entry, "../outside.weave"),
            Err(ResolveError::OutsideRoot(_))
        ));
        assert!(matches!(
            resolver.resolve(&entry, "outside.weave"),
            Err(ResolveError::InvalidPath(_))
        ));
        let _ = fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn symlink_escaping_the_root_is_outside_root() {
        let dir = temp_dir("symlink");
        fs::create_dir_all(dir.join("root")).unwrap();
        fs::write(dir.join("root/main.weave"), "m").unwrap();
        fs::write(dir.join("secret.weave"), "s").unwrap();
        std::os::unix::fs::symlink(dir.join("secret.weave"), dir.join("root/link.weave")).unwrap();
        let (mut resolver, entry) = FsResolver::for_entry(&dir.join("root/main.weave")).unwrap();
        assert!(matches!(
            resolver.resolve(&entry, "./link.weave"),
            Err(ResolveError::OutsideRoot(_))
        ));
        let link = ModuleId::new("link.weave").unwrap();
        assert!(matches!(
            resolver.load(&link, 100),
            Err(ResolveError::OutsideRoot(_))
        ));
        let _ = fs::remove_dir_all(&dir);
    }
}
