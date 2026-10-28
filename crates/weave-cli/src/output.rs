//! Output files: atomic replacement, file identity and path formatting.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

/// Replaces `path` with `bytes` atomically: the bytes are written and synced
/// to a new temporary file next to `path`, which is then renamed over it.
/// Readers see either the old file or the new one, never a partial write;
/// on failure the old file is left untouched and the temporary file is
/// removed.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    static COUNTER: AtomicU32 = AtomicU32::new(0);
    let name = path
        .file_name()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "no file name"))?;
    let dir = match path.parent() {
        Some(dir) if !dir.as_os_str().is_empty() => dir,
        _ => Path::new("."),
    };
    let mut temp_name = std::ffi::OsString::from(".");
    temp_name.push(name);
    temp_name.push(format!(
        ".{}-{}.weave-tmp",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    let temp = dir.join(temp_name);
    let result = (|| {
        let mut file: File = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        fs::rename(&temp, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result
}

/// The identity of an output path for collision checks: the canonical path
/// of an existing file (following symbolic links), otherwise the canonical
/// parent directory joined with the file name.
pub fn identity(path: &Path) -> io::Result<PathBuf> {
    if let Ok(canonical) = path.canonicalize() {
        return Ok(canonical);
    }
    let name = path
        .file_name()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "no file name"))?;
    let dir = match path.parent() {
        Some(dir) if !dir.as_os_str().is_empty() => dir,
        _ => Path::new("."),
    };
    Ok(dir.canonicalize()?.join(name))
}

/// Whether two identities name the same file. Windows and macOS file
/// systems are usually case-insensitive, so the comparison ignores ASCII
/// case there (a conservative choice: it may reject a valid case-only
/// difference on a case-sensitive volume).
pub fn same_file(a: &Path, b: &Path) -> bool {
    if cfg!(any(windows, target_os = "macos")) {
        a.as_os_str()
            .to_string_lossy()
            .eq_ignore_ascii_case(&b.as_os_str().to_string_lossy())
    } else {
        a == b
    }
}

/// The path of `to` relative to the directory `from`, both absolute and
/// canonical, written with `/` separators. `None` when the paths have no
/// common root (for example different Windows drives) or are not UTF-8.
pub fn relative(from: &Path, to: &Path) -> Option<String> {
    let from: Vec<Component<'_>> = from.components().collect();
    let to: Vec<Component<'_>> = to.components().collect();
    let common = from
        .iter()
        .zip(&to)
        .take_while(|(a, b)| components_equal(a, b))
        .count();
    // The prefix (drive or `/`) must be shared.
    if common == 0 {
        return None;
    }
    let mut parts: Vec<&str> = vec![".."; from.len() - common];
    for component in to.get(common..)? {
        match component {
            Component::Normal(part) => parts.push(part.to_str()?),
            _ => return None,
        }
    }
    Some(parts.join("/"))
}

fn components_equal(a: &Component<'_>, b: &Component<'_>) -> bool {
    if cfg!(any(windows, target_os = "macos")) {
        a.as_os_str()
            .to_string_lossy()
            .eq_ignore_ascii_case(&b.as_os_str().to_string_lossy())
    } else {
        a == b
    }
}

/// Standard Base64 with padding (RFC 4648 §4).
pub fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let digit = |index: u32| char::from(ALPHABET.get(index as usize & 63).copied().unwrap_or(b'A'));
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [
            chunk.first().copied().unwrap_or(0),
            chunk.get(1).copied().unwrap_or(0),
            chunk.get(2).copied().unwrap_or(0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        out.push(digit(n >> 18));
        out.push(digit(n >> 12));
        out.push(if chunk.len() > 1 { digit(n >> 6) } else { '=' });
        out.push(if chunk.len() > 2 { digit(n) } else { '=' });
    }
    out
}

/// Percent-encodes a relative URL path: everything except unreserved
/// characters and `/` is encoded, so the result cannot end a CSS comment.
pub fn url_path(path: &str) -> String {
    let mut out = String::with_capacity(path.len());
    for byte in path.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~' | b'/') {
            out.push(char::from(byte));
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_matches_rfc_4648_vectors() {
        for (input, expected) in [
            ("", ""),
            ("f", "Zg=="),
            ("fo", "Zm8="),
            ("foo", "Zm9v"),
            ("foob", "Zm9vYg=="),
            ("fooba", "Zm9vYmE="),
            ("foobar", "Zm9vYmFy"),
        ] {
            assert_eq!(base64(input.as_bytes()), expected, "{input}");
        }
        assert_eq!(base64(&[0xff, 0xfe, 0xfd]), "//79");
    }

    #[test]
    fn relative_paths() {
        let root = if cfg!(windows) { "C:\\p" } else { "/p" };
        let p = |s: &str| {
            PathBuf::from(format!(
                "{root}{}",
                s.replace('/', std::path::MAIN_SEPARATOR_STR)
            ))
        };
        assert_eq!(
            relative(&p("/out"), &p("/src/a.weave")).as_deref(),
            Some("../src/a.weave")
        );
        assert_eq!(
            relative(&p("/src"), &p("/src/ui/a.weave")).as_deref(),
            Some("ui/a.weave")
        );
        assert_eq!(
            relative(&p("/a/b/c"), &p("/x.weave")).as_deref(),
            Some("../../../x.weave")
        );
        if cfg!(windows) {
            assert_eq!(relative(Path::new("C:\\a"), Path::new("D:\\b.weave")), None);
        }
    }

    #[test]
    fn url_paths_cannot_close_a_comment() {
        assert_eq!(url_path("out dir/a*b.css.map"), "out%20dir/a%2Ab.css.map");
        assert_eq!(url_path("é.map"), "%C3%A9.map");
    }

    #[test]
    fn atomic_write_replaces_and_cleans_up() {
        let dir = std::env::temp_dir().join(format!("weave-atomic-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let target = dir.join("out.css");
        write_atomic(&target, b"one").unwrap();
        write_atomic(&target, b"two").unwrap();
        assert_eq!(fs::read(&target).unwrap(), b"two");
        // A missing directory fails without creating anything.
        assert!(write_atomic(&dir.join("missing/out.css"), b"x").is_err());
        let names: Vec<_> = fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(names, [std::ffi::OsString::from("out.css")]);
        let _ = fs::remove_dir_all(&dir);
    }
}
