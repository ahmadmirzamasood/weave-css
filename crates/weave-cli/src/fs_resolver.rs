//! Filesystem module resolver rooted at a directory (LANGUAGE.md §14).
//!
//! Module identities are canonical paths relative to the root, so symbolic
//! links that point outside the root are rejected (E7006). Checks use
//! ordinary paths: a concurrent filesystem change between the check and the
//! read is not defended against.
//!
//! A resolver serves one build. It canonicalizes each path once and reuses
//! the result for the rest of the build (canonicalization dominated module
//! loading in the phase 09 measurements); every build starts with a new
//! resolver, so a later build sees the file system as it is then.

use std::collections::{BTreeSet, HashMap};
use std::fs::File;
use std::io::{self, Read};
use std::path::{Component, Path, PathBuf};

use weave_compiler::{LoadedModule, ModuleId, ModuleResolver, ResolveError, resolve};

/// The module identity of standard input. `<` cannot appear in a Windows
/// file name; elsewhere a file with this exact name is shadowed by stdin.
pub const STDIN_MODULE: &str = "<stdin>.weave";

#[derive(Debug)]
pub struct FsResolver {
    /// Canonical root directory.
    root: PathBuf,
    /// The entry read from standard input, if any.
    stdin: Option<(ModuleId, Vec<u8>)>,
    /// Every filesystem path a build looked at: lexical paths of `load`
    /// specifiers (even when missing) and canonical paths of loaded modules.
    touched: BTreeSet<PathBuf>,
    /// Canonicalization results of this build, by the module path that was
    /// canonicalized.
    canonical: HashMap<ModuleId, Result<PathBuf, ResolveError>>,
}

impl FsResolver {
    /// Creates a resolver rooted at the entry file's directory and returns
    /// it with the entry's module identity.
    pub fn for_entry(entry: &Path) -> io::Result<(FsResolver, ModuleId)> {
        check_extension(entry)?;
        let canonical = entry.canonicalize()?;
        let parent = canonical
            .parent()
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "no parent directory"))?;
        let resolver = FsResolver::with_root(parent)?;
        let id = resolver.entry_id(entry)?;
        Ok((resolver, id))
    }

    /// Creates a resolver rooted at the directory `root`.
    pub fn with_root(root: &Path) -> io::Result<FsResolver> {
        let root = root.canonicalize()?;
        if !root.is_dir() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "the module root is not a directory",
            ));
        }
        Ok(FsResolver {
            root,
            stdin: None,
            touched: BTreeSet::new(),
            canonical: HashMap::new(),
        })
    }

    /// The module identity of the entry file `entry`, which must be a
    /// `.weave` file inside the root.
    pub fn entry_id(&self, entry: &Path) -> io::Result<ModuleId> {
        check_extension(entry)?;
        let canonical = entry.canonicalize()?;
        if !canonical.is_file() {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "not a file"));
        }
        self.module_id(&canonical).map_err(|err| {
            let message = match err {
                ResolveError::OutsideRoot(_) => {
                    "the input file is outside the module root".to_string()
                }
                other => other.to_string(),
            };
            io::Error::new(io::ErrorKind::InvalidInput, message)
        })
    }

    /// Makes standard input the entry module and returns its identity.
    pub fn set_stdin(&mut self, bytes: Vec<u8>) -> ModuleId {
        // `STDIN_MODULE` is a valid module path.
        let id = ModuleId::new(STDIN_MODULE).unwrap_or_else(|_| unreachable!());
        self.stdin = Some((id.clone(), bytes));
        id
    }

    /// The filesystem path of a module (not canonicalized), or `None` for
    /// standard input.
    pub fn path(&self, id: &ModuleId) -> Option<PathBuf> {
        if self.is_stdin(id) {
            return None;
        }
        Some(self.path_of(id))
    }

    /// The canonical path of a module (canonicalized at most once per
    /// build), or `None` for standard input. A path that cannot be
    /// canonicalized is returned as is.
    pub fn canonical_path(&mut self, id: &ModuleId) -> Option<PathBuf> {
        if self.is_stdin(id) {
            return None;
        }
        Some(self.canonical(id).unwrap_or_else(|_| self.path_of(id)))
    }

    /// Returns and forgets the paths looked at since the last call.
    pub fn take_touched(&mut self) -> BTreeSet<PathBuf> {
        std::mem::take(&mut self.touched)
    }

    fn is_stdin(&self, id: &ModuleId) -> bool {
        self.stdin.as_ref().is_some_and(|(stdin, _)| stdin == id)
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

    /// Canonicalizes the path of `id`, following symbolic links, once per
    /// build.
    fn canonical(&mut self, id: &ModuleId) -> Result<PathBuf, ResolveError> {
        if let Some(known) = self.canonical.get(id) {
            return known.clone();
        }
        let result = self
            .path_of(id)
            .canonicalize()
            .map_err(|err| io_error(id, &err));
        self.canonical.insert(id.clone(), result.clone());
        result
    }
}

fn check_extension(entry: &Path) -> io::Result<()> {
    if entry.extension().is_none_or(|ext| ext != "weave") {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "input files must have the .weave extension",
        ));
    }
    Ok(())
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
        self.touched.insert(self.path_of(&lexical));
        let canonical = self.canonical(&lexical)?;
        self.module_id(&canonical)
    }

    fn load(&mut self, id: &ModuleId, max_bytes: u64) -> Result<LoadedModule, ResolveError> {
        if let Some((_, bytes)) = self.stdin.as_ref().filter(|(stdin, _)| stdin == id) {
            let keep = usize::try_from(max_bytes.saturating_add(1)).unwrap_or(usize::MAX);
            return Ok(LoadedModule {
                display_name: id.to_string(),
                bytes: bytes.iter().take(keep).copied().collect(),
            });
        }
        self.touched.insert(self.path_of(id));
        let canonical = self.canonical(id)?;
        self.touched.insert(canonical.clone());
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

    #[test]
    fn identity_is_canonical_across_spellings() {
        let dir = temp_dir("identity");
        fs::create_dir_all(dir.join("a/b")).unwrap();
        fs::write(dir.join("main.weave"), "m").unwrap();
        fs::write(dir.join("a/x.weave"), "x").unwrap();
        let (mut resolver, entry) = FsResolver::for_entry(&dir.join("main.weave")).unwrap();
        let direct = resolver.resolve(&entry, "./a/x.weave").unwrap();
        let roundabout = resolver.resolve(&entry, "./a/b/../x.weave").unwrap();
        assert_eq!(direct, roundabout);
        assert_eq!(direct.as_str(), "a/x.weave");
        // From a subdirectory, `..` may return to the root but not leave it.
        let from_sub = ModuleId::new("a/x.weave").unwrap();
        assert_eq!(resolver.resolve(&from_sub, "../main.weave").unwrap(), entry);
        assert!(matches!(
            resolver.resolve(&from_sub, "../../main.weave"),
            Err(ResolveError::OutsideRoot(_))
        ));
        let _ = fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn symlinks_inside_the_root_share_the_target_identity() {
        let dir = temp_dir("inner-link");
        fs::create_dir_all(dir.join("real")).unwrap();
        fs::write(dir.join("main.weave"), "m").unwrap();
        fs::write(dir.join("real/t.weave"), "t").unwrap();
        std::os::unix::fs::symlink(dir.join("real/t.weave"), dir.join("alias.weave")).unwrap();
        std::os::unix::fs::symlink(dir.join("real"), dir.join("linkdir")).unwrap();
        let (mut resolver, entry) = FsResolver::for_entry(&dir.join("main.weave")).unwrap();
        let target = ModuleId::new("real/t.weave").unwrap();
        assert_eq!(resolver.resolve(&entry, "./alias.weave").unwrap(), target);
        assert_eq!(
            resolver.resolve(&entry, "./linkdir/t.weave").unwrap(),
            target
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn directory_symlink_escaping_the_root_is_outside_root() {
        let dir = temp_dir("dir-link");
        fs::create_dir_all(dir.join("root")).unwrap();
        fs::create_dir_all(dir.join("outside")).unwrap();
        fs::write(dir.join("root/main.weave"), "m").unwrap();
        fs::write(dir.join("outside/s.weave"), "s").unwrap();
        std::os::unix::fs::symlink(dir.join("outside"), dir.join("root/out")).unwrap();
        let (mut resolver, entry) = FsResolver::for_entry(&dir.join("root/main.weave")).unwrap();
        assert!(matches!(
            resolver.resolve(&entry, "./out/s.weave"),
            Err(ResolveError::OutsideRoot(_))
        ));
        let _ = fs::remove_dir_all(&dir);
    }

    /// Creating symbolic links on Windows needs a privilege (or developer
    /// mode); the test checks the escape when the link can be created and
    /// is skipped otherwise.
    #[cfg(windows)]
    #[test]
    fn symlink_escaping_the_root_is_outside_root_on_windows() {
        let dir = temp_dir("win-symlink");
        fs::create_dir_all(dir.join("root")).unwrap();
        fs::write(dir.join("root/main.weave"), "m").unwrap();
        fs::write(dir.join("secret.weave"), "s").unwrap();
        if std::os::windows::fs::symlink_file(dir.join("secret.weave"), dir.join("root/link.weave"))
            .is_err()
        {
            eprintln!("skipped: cannot create symbolic links here");
            let _ = fs::remove_dir_all(&dir);
            return;
        }
        let (mut resolver, entry) = FsResolver::for_entry(&dir.join("root/main.weave")).unwrap();
        assert!(matches!(
            resolver.resolve(&entry, "./link.weave"),
            Err(ResolveError::OutsideRoot(_))
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
