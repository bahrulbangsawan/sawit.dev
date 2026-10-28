//! Project files for the text editor: directory listing, safe reading
//! (size and encoding limits) and atomic saving.

use std::path::{Path, PathBuf};

use crate::Result;

/// Files larger than this open as a read-only notice instead of text.
pub const MAX_TEXT_BYTES: u64 = 5 * 1024 * 1024;

const HIDDEN: [&str; 6] = [
    ".git",
    "node_modules",
    "target",
    ".DS_Store",
    ".superpowers",
    ".turbo",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub name: String,
    pub path: PathBuf,
    pub is_dir: bool,
}

/// Directory contents: folders first, then files, each case-insensitively
/// sorted; VCS/build noise hidden.
pub fn list_dir(dir: &Path) -> Result<Vec<Entry>> {
    let mut entries: Vec<Entry> = std::fs::read_dir(dir)?
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let name = e.file_name().into_string().ok()?;
            if HIDDEN.contains(&name.as_str()) {
                return None;
            }
            let is_dir = e.file_type().ok()?.is_dir() || e.path().is_dir();
            Some(Entry {
                name,
                path: e.path(),
                is_dir,
            })
        })
        .collect();
    entries.sort_by(|a, b| {
        b.is_dir
            .cmp(&a.is_dir)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    Ok(entries)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextFile {
    pub text: String,
    /// Why the file can't be edited (too big, binary); `text` is empty then.
    pub read_only_reason: Option<String>,
}

pub fn read_text(path: &Path) -> Result<TextFile> {
    let size = std::fs::metadata(path)?.len();
    if size > MAX_TEXT_BYTES {
        return Ok(TextFile {
            text: String::new(),
            read_only_reason: Some(format!(
                "File {:.1} MB terlalu besar untuk editor (batas {} MB).",
                size as f64 / 1_048_576.0,
                MAX_TEXT_BYTES / 1_048_576
            )),
        });
    }
    let bytes = std::fs::read(path)?;
    match String::from_utf8(bytes) {
        Ok(text) if !text.contains('\0') => Ok(TextFile {
            text,
            read_only_reason: None,
        }),
        _ => Ok(TextFile {
            text: String::new(),
            read_only_reason: Some(
                "File biner atau bukan UTF-8; tidak dibuka sebagai teks.".into(),
            ),
        }),
    }
}

/// Writes via a temp file in the same directory + rename, keeping the
/// original file's permissions.
pub fn write_text_atomic(path: &Path, text: &str) -> Result<()> {
    let dir = path.parent().unwrap_or(Path::new("."));
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let tmp = dir.join(format!(".{name}.pgdesk-tmp"));
    std::fs::write(&tmp, text)?;
    if let Ok(meta) = std::fs::metadata(path) {
        std::fs::set_permissions(&tmp, meta.permissions())?;
    }
    std::fs::rename(&tmp, path)?;
    Ok(())
}

/// Highlighter language name (gpui-component's names) for a file.
pub fn language_for(path: &Path) -> &'static str {
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_default();
    if name.starts_with(".env") {
        return "bash";
    }
    match path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or_default()
    {
        "sql" | "psql" => "sql",
        "ts" | "mts" | "cts" => "typescript",
        "tsx" => "tsx",
        "js" | "mjs" | "cjs" | "jsx" => "javascript",
        "json" | "jsonc" => "json",
        "md" | "mdx" => "markdown",
        "toml" => "toml",
        "rs" => "rust",
        "css" | "scss" => "css",
        "html" | "htm" => "html",
        "yml" | "yaml" => "yaml",
        "sh" | "bash" | "zsh" => "bash",
        "go" => "go",
        "py" => "python",
        _ => "text",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn list_dir_sorts_dirs_first_and_hides_noise() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path();
        for dir in ["src", ".git", "node_modules", "target", "Assets"] {
            fs::create_dir(p.join(dir)).unwrap();
        }
        for file in ["b.ts", "a.sql", ".env", ".DS_Store"] {
            fs::write(p.join(file), "").unwrap();
        }
        let names: Vec<_> = list_dir(p)
            .unwrap()
            .into_iter()
            .map(|e| (e.name, e.is_dir))
            .collect();
        assert_eq!(
            names,
            vec![
                ("Assets".into(), true),
                ("src".into(), true),
                (".env".into(), false),
                ("a.sql".into(), false),
                ("b.ts".into(), false),
            ]
        );
    }

    #[test]
    fn reads_utf8_text() {
        let d = tempfile::tempdir().unwrap();
        let f = d.path().join("q.sql");
        fs::write(&f, "select 1;\n").unwrap();
        let t = read_text(&f).unwrap();
        assert_eq!(t.text, "select 1;\n");
        assert_eq!(t.read_only_reason, None);
    }

    #[test]
    fn binary_file_is_not_opened_as_text() {
        let d = tempfile::tempdir().unwrap();
        let f = d.path().join("img.png");
        fs::write(&f, [0x89, b'P', b'N', b'G', 0, 0, 0xff, 0xfe]).unwrap();
        let t = read_text(&f).unwrap();
        assert!(t.text.is_empty());
        assert!(t.read_only_reason.unwrap().contains("biner"));
    }

    #[test]
    fn huge_file_is_refused() {
        let d = tempfile::tempdir().unwrap();
        let f = d.path().join("big.log");
        fs::write(&f, vec![b'a'; MAX_TEXT_BYTES as usize + 1]).unwrap();
        let t = read_text(&f).unwrap();
        assert!(t.text.is_empty());
        assert!(t.read_only_reason.unwrap().contains("MB"));
    }

    #[test]
    fn write_is_atomic_and_preserves_permissions() {
        use std::os::unix::fs::PermissionsExt;
        let d = tempfile::tempdir().unwrap();
        let f = d.path().join("run.sh");
        fs::write(&f, "old").unwrap();
        fs::set_permissions(&f, fs::Permissions::from_mode(0o755)).unwrap();
        write_text_atomic(&f, "new").unwrap();
        assert_eq!(fs::read_to_string(&f).unwrap(), "new");
        assert_eq!(
            fs::metadata(&f).unwrap().permissions().mode() & 0o777,
            0o755
        );
        let leftovers: Vec<_> = fs::read_dir(d.path()).unwrap().collect();
        assert_eq!(leftovers.len(), 1);
    }

    #[test]
    fn language_from_extension() {
        assert_eq!(language_for(Path::new("a/b.sql")), "sql");
        assert_eq!(language_for(Path::new("x.tsx")), "tsx");
        assert_eq!(language_for(Path::new("x.mjs")), "javascript");
        assert_eq!(language_for(Path::new("Cargo.toml")), "toml");
        assert_eq!(language_for(Path::new("README.md")), "markdown");
        assert_eq!(language_for(Path::new(".env")), "bash");
        assert_eq!(language_for(Path::new("noext")), "text");
    }
}
