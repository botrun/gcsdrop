use anyhow::{bail, Result};
use std::path::{Path, PathBuf};

/// Upper bounds, mirroring what a browser-facing report needs.
/// Well below any GCS limit; these exist to catch mistakes like
/// pointing gcsdrop at a home directory.
const MAX_FILES: usize = 2000;
const MAX_TOTAL_BYTES: u64 = 512 * 1024 * 1024;

#[derive(Debug, Clone)]
pub struct Entry {
    /// Absolute or relative path on the local filesystem.
    pub local: PathBuf,
    /// Path inside the upload, using forward slashes. Never starts with '/'.
    pub relative: String,
    pub content_type: String,
    pub size: u64,
}

#[derive(Debug, Clone)]
pub struct Manifest {
    pub entries: Vec<Entry>,
    pub total_bytes: u64,
    /// The entry a viewer should land on: `index.html` if present, or the
    /// single file when the input was one file. `None` means the caller
    /// must let the viewer pick.
    pub index: Option<String>,
}

pub fn content_type_for(path: &Path) -> String {
    mime_guess::from_path(path)
        .first_raw()
        .unwrap_or("application/octet-stream")
        .to_string()
}

pub fn random_suffix() -> String {
    use rand::RngExt;
    const ALPHABET: &[u8] = b"abcdefghijklmnopqrstuvwxyz0123456789";
    let mut rng = rand::rng();
    (0..8)
        .map(|_| ALPHABET[rng.random_range(0..ALPHABET.len())] as char)
        .collect()
}

pub fn run_id(now: chrono::DateTime<chrono::Utc>, rand8: &str) -> String {
    format!("{}-{}", now.format("%Y%m%d-%H%M%S"), rand8)
}

pub fn scan(path: &Path) -> Result<Manifest> {
    if !path.exists() {
        bail!("{} does not exist", path.display());
    }

    let entries = if path.is_file() {
        let name = path
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "index.html".to_string());
        vec![entry_for(path.to_path_buf(), name)?]
    } else {
        let mut out = Vec::new();
        for e in walkdir::WalkDir::new(path)
            .into_iter()
            .filter_entry(|e| !is_hidden(e))
        {
            let e = e?;
            if !e.file_type().is_file() {
                continue;
            }
            let rel = e
                .path()
                .strip_prefix(path)?
                .components()
                .map(|c| c.as_os_str().to_string_lossy().to_string())
                .collect::<Vec<_>>()
                .join("/");
            out.push(entry_for(e.path().to_path_buf(), rel)?);
        }
        out
    };

    if entries.is_empty() {
        bail!("{} contains no files to upload", path.display());
    }
    if entries.len() > MAX_FILES {
        bail!(
            "{} contains {} files, more than the {MAX_FILES} gcsdrop uploads at once",
            path.display(),
            entries.len()
        );
    }

    let total_bytes: u64 = entries.iter().map(|e| e.size).sum();
    if total_bytes > MAX_TOTAL_BYTES {
        bail!(
            "{} is {total_bytes} bytes, more than the {MAX_TOTAL_BYTES} byte limit",
            path.display()
        );
    }

    let index = if entries.len() == 1 {
        Some(entries[0].relative.clone())
    } else {
        entries
            .iter()
            .map(|e| e.relative.clone())
            .find(|r| r == "index.html")
    };

    Ok(Manifest {
        entries,
        total_bytes,
        index,
    })
}

fn entry_for(local: PathBuf, relative: String) -> Result<Entry> {
    let size = std::fs::metadata(&local)?.len();
    let content_type = content_type_for(&local);
    Ok(Entry {
        local,
        relative,
        content_type,
        size,
    })
}

fn is_hidden(e: &walkdir::DirEntry) -> bool {
    // The root itself may legitimately be a dotted path the user typed.
    if e.depth() == 0 {
        return false;
    }
    e.file_name().to_string_lossy().starts_with('.')
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn content_type_covers_web_assets() {
        let cases = [
            ("a.html", "text/html"),
            ("a.htm", "text/html"),
            ("a.css", "text/css"),
            ("a.js", "text/javascript"),
            ("a.json", "application/json"),
            ("a.png", "image/png"),
            ("a.jpg", "image/jpeg"),
            ("a.svg", "image/svg+xml"),
            ("a.woff2", "font/woff2"),
        ];
        for (name, expected) in cases {
            let got = content_type_for(Path::new(name));
            assert!(
                got.starts_with(expected),
                "{name}: expected {expected}, got {got}"
            );
        }
    }

    #[test]
    fn unknown_extension_falls_back_to_octet_stream() {
        assert_eq!(
            content_type_for(Path::new("a.wat")),
            "application/octet-stream"
        );
    }

    #[test]
    fn no_extension_falls_back_to_octet_stream() {
        assert_eq!(
            content_type_for(Path::new("README")),
            "application/octet-stream"
        );
    }

    #[test]
    fn run_id_is_timestamp_plus_random() {
        let t = chrono::DateTime::parse_from_rfc3339("2026-09-03T04:05:06Z")
            .unwrap()
            .with_timezone(&chrono::Utc);
        assert_eq!(run_id(t, "abcd1234"), "20260903-040506-abcd1234");
    }

    #[test]
    fn random_suffix_is_eight_lowercase_alphanumerics() {
        let s = random_suffix();
        assert_eq!(s.len(), 8);
        assert!(s
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit()));
    }

    #[test]
    fn random_suffix_differs_between_calls() {
        assert_ne!(random_suffix(), random_suffix());
    }

    #[test]
    fn scan_single_file() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("report.html");
        std::fs::write(&f, "<h1>hi</h1>").unwrap();

        let m = scan(&f).unwrap();
        assert_eq!(m.entries.len(), 1);
        assert_eq!(m.entries[0].relative, "report.html");
        assert!(m.entries[0].content_type.starts_with("text/html"));
        assert_eq!(m.total_bytes, 11);
        assert_eq!(m.index.as_deref(), Some("report.html"));
    }

    #[test]
    fn scan_directory_keeps_relative_paths() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("index.html"), "<h1>hi</h1>").unwrap();
        std::fs::create_dir(dir.path().join("assets")).unwrap();
        std::fs::write(dir.path().join("assets/style.css"), "body{}").unwrap();

        let m = scan(dir.path()).unwrap();
        let mut names: Vec<_> = m.entries.iter().map(|e| e.relative.clone()).collect();
        names.sort();
        assert_eq!(names, vec!["assets/style.css", "index.html"]);
        assert_eq!(m.index.as_deref(), Some("index.html"));
    }

    #[test]
    fn scan_skips_hidden_files_and_dirs() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("index.html"), "x").unwrap();
        std::fs::write(dir.path().join(".DS_Store"), "x").unwrap();
        std::fs::create_dir(dir.path().join(".git")).unwrap();
        std::fs::write(dir.path().join(".git/HEAD"), "x").unwrap();

        let m = scan(dir.path()).unwrap();
        let names: Vec<_> = m.entries.iter().map(|e| e.relative.clone()).collect();
        assert_eq!(names, vec!["index.html"]);
    }

    #[test]
    fn scan_directory_without_index_has_no_index() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.html"), "x").unwrap();
        std::fs::write(dir.path().join("b.html"), "x").unwrap();

        let m = scan(dir.path()).unwrap();
        assert_eq!(m.index, None);
    }

    #[test]
    fn scan_rejects_empty_directory() {
        let dir = tempfile::tempdir().unwrap();
        let err = scan(dir.path()).unwrap_err().to_string();
        assert!(err.contains("no files"), "got: {err}");
    }

    #[test]
    fn scan_rejects_missing_path() {
        let err = scan(Path::new("/nope/does/not/exist"))
            .unwrap_err()
            .to_string();
        assert!(err.contains("does not exist"), "got: {err}");
    }
}
