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
    let meta = std::fs::metadata(path)?;
    if !meta.is_file() {
        // FIFOs, sockets and devices: opening them can block or never end.
        return Ok(TextFile {
            text: String::new(),
            read_only_reason: Some("Bukan file biasa (pipe, socket, atau device).".into()),
        });
    }
    let size = meta.len();
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

/// Writes via a private temp file in the same directory + rename, keeping
/// the original file's permissions. A symlink stays a symlink: its target is
/// what gets replaced.
pub fn write_text_atomic(path: &Path, text: &str) -> Result<()> {
    use std::io::Write as _;
    use std::os::unix::fs::OpenOptionsExt as _;

    let path = match std::fs::symlink_metadata(path) {
        Ok(m) if m.file_type().is_symlink() => std::fs::canonicalize(path)?,
        _ => path.to_path_buf(),
    };
    let dir = path.parent().unwrap_or(Path::new("."));
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let tmp = dir.join(format!(".{name}.pgdesk-tmp"));
    let result = (|| {
        // Owner-only until the final permissions are applied.
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&tmp)?;
        file.write_all(text.as_bytes())?;
        file.sync_all()?;
        let mode = std::fs::metadata(&path)
            .map(|m| m.permissions())
            .unwrap_or_else(|_| std::os::unix::fs::PermissionsExt::from_mode(0o644));
        std::fs::set_permissions(&tmp, mode)?;
        std::fs::rename(&tmp, &path)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    Ok(result?)
}

/// What the center pane shows for a file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileKind {
    /// Code or plain text: the editor.
    Text,
    /// Editor plus rendered preview.
    Markdown,
    Html,
    Svg,
    /// Viewers.
    Image,
    Pdf,
    Video,
}

fn extension(path: &Path) -> String {
    path.extension()
        .and_then(|e| e.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase()
}

pub fn classify(path: &Path) -> FileKind {
    match extension(path).as_str() {
        "md" | "mdx" | "markdown" => FileKind::Markdown,
        "html" | "htm" => FileKind::Html,
        "svg" => FileKind::Svg,
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" | "ico" | "tif" | "tiff" | "heic"
        | "heif" | "avif" => FileKind::Image,
        "pdf" => FileKind::Pdf,
        "mp4" | "m4v" | "mov" | "webm" | "mkv" | "avi" => FileKind::Video,
        _ => FileKind::Text,
    }
}

/// Highlighter language token for a file: a syntax name the editor's
/// syntect backend knows, or the bare extension for it to try.
pub fn language_for(path: &Path) -> String {
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_default();
    let lower = name.to_ascii_lowercase();
    if lower.starts_with(".env") {
        return "bash".into();
    }
    match lower.as_str() {
        "dockerfile" | "containerfile" => return "dockerfile".into(),
        "makefile" | "gnumakefile" => return "make".into(),
        ".bashrc" | ".zshrc" | ".profile" | ".bash_profile" | ".zprofile" => {
            return "bash".into();
        }
        "cargo.lock" => return "toml".into(),
        "yarn.lock" => return "yaml".into(),
        _ => {}
    }
    let ext = extension(path);
    let lang = match ext.as_str() {
        "" => "text",
        "dockerfile" => "dockerfile",
        "sql" | "psql" => "sql",
        "ts" | "mts" | "cts" => "typescript",
        "tsx" => "tsx",
        "js" | "mjs" | "cjs" | "jsx" => "javascript",
        "json" | "jsonc" | "json5" | "lock" => "json",
        "md" | "mdx" | "markdown" => "markdown",
        "toml" => "toml",
        "rs" => "rust",
        "css" => "css",
        "scss" | "sass" => "scss",
        "less" => "less",
        "html" | "htm" => "html",
        "xml" | "svg" | "plist" => "xml",
        "yml" | "yaml" => "yaml",
        "sh" | "bash" | "zsh" | "fish" => "bash",
        "go" => "go",
        "py" | "pyi" => "python",
        "kt" | "kts" => "kotlin",
        "java" => "java",
        "c" | "h" => "c",
        "cc" | "cpp" | "cxx" | "hpp" | "hh" | "hxx" => "cpp",
        "cs" => "csharp",
        "rb" => "ruby",
        "php" => "php",
        "swift" => "swift",
        "lua" => "lua",
        "zig" => "zig",
        "vue" => "vue",
        "svelte" => "svelte",
        "graphql" | "gql" => "graphql",
        "ini" | "cfg" | "conf" => "ini",
        "diff" | "patch" => "diff",
        "prisma" => "prisma",
        "txt" | "log" => "text",
        other => return other.to_string(),
    };
    lang.into()
}

/// Human name of a language token, for the editor's status line.
pub fn language_label(token: &str) -> String {
    let label = match token {
        "text" => "Teks",
        "sql" => "SQL",
        "typescript" => "TypeScript",
        "tsx" => "TSX",
        "javascript" => "JavaScript",
        "json" => "JSON",
        "markdown" => "Markdown",
        "toml" => "TOML",
        "rust" => "Rust",
        "css" => "CSS",
        "scss" => "SCSS",
        "less" => "Less",
        "html" => "HTML",
        "xml" => "XML",
        "yaml" => "YAML",
        "bash" => "Shell",
        "go" => "Go",
        "python" => "Python",
        "kotlin" => "Kotlin",
        "java" => "Java",
        "c" => "C",
        "cpp" => "C++",
        "csharp" => "C#",
        "ruby" => "Ruby",
        "php" => "PHP",
        "swift" => "Swift",
        "lua" => "Lua",
        "zig" => "Zig",
        "vue" => "Vue",
        "svelte" => "Svelte",
        "graphql" => "GraphQL",
        "ini" => "INI",
        "diff" => "Diff",
        "prisma" => "Prisma",
        "dockerfile" => "Dockerfile",
        "make" => "Makefile",
        other => return other.to_string(),
    };
    label.into()
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

    #[test]
    fn language_covers_common_project_files() {
        for (file, lang) in [
            ("Dockerfile", "dockerfile"),
            ("api.Dockerfile", "dockerfile"),
            ("Makefile", "make"),
            ("GNUmakefile", "make"),
            (".env.local", "bash"),
            (".zshrc", "bash"),
            ("x.TSX", "tsx"),
            ("x.vue", "vue"),
            ("x.svelte", "svelte"),
            ("x.kt", "kotlin"),
            ("x.java", "java"),
            ("x.c", "c"),
            ("x.hpp", "cpp"),
            ("x.cs", "csharp"),
            ("x.rb", "ruby"),
            ("x.php", "php"),
            ("x.swift", "swift"),
            ("x.lua", "lua"),
            ("x.zig", "zig"),
            ("x.scss", "scss"),
            ("x.less", "less"),
            ("x.graphql", "graphql"),
            ("x.xml", "xml"),
            ("x.svg", "xml"),
            ("x.ini", "ini"),
            ("x.diff", "diff"),
            ("x.prisma", "prisma"),
            ("tsconfig.json", "json"),
            ("x.jsonc", "json"),
            ("Cargo.lock", "toml"),
            ("bun.lock", "json"),
            ("yarn.lock", "yaml"),
        ] {
            assert_eq!(language_for(Path::new(file)), lang, "{file}");
        }
        // Unknown extensions are passed through for the highlighter to try.
        assert_eq!(language_for(Path::new("x.nim")), "nim");
    }

    #[test]
    fn language_labels_for_the_status_line() {
        assert_eq!(language_label("tsx"), "TSX");
        assert_eq!(language_label("rust"), "Rust");
        assert_eq!(language_label("text"), "Teks");
        assert_eq!(language_label("nim"), "nim");
    }

    #[test]
    fn file_kind_picks_the_viewer() {
        use FileKind::*;
        for (file, kind) in [
            ("a.ts", Text),
            ("README.md", Markdown),
            ("x.MDX", Markdown),
            ("index.html", Html),
            ("logo.svg", Svg),
            ("a.png", Image),
            ("a.JPG", Image),
            ("a.webp", Image),
            ("a.gif", Image),
            ("a.heic", Image),
            ("doc.pdf", Pdf),
            ("clip.mp4", Video),
            ("clip.mov", Video),
            ("clip.webm", Video),
            ("noext", Text),
        ] {
            assert_eq!(classify(Path::new(file)), kind, "{file}");
        }
    }

    #[test]
    fn special_files_are_not_read() {
        let d = tempfile::tempdir().unwrap();
        let fifo = d.path().join("pipe");
        assert!(
            std::process::Command::new("mkfifo")
                .arg(&fifo)
                .status()
                .unwrap()
                .success()
        );
        // Opening a FIFO for reading blocks until a writer appears; this must not.
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || tx.send(read_text(&fifo)).unwrap());
        let file = rx
            .recv_timeout(std::time::Duration::from_secs(2))
            .expect("read_text blocked on a FIFO")
            .unwrap();
        assert!(file.read_only_reason.is_some());
    }

    #[test]
    fn saving_through_a_symlink_keeps_the_link() {
        let d = tempfile::tempdir().unwrap();
        let target = d.path().join("real.txt");
        let link = d.path().join("link.txt");
        fs::write(&target, "old").unwrap();
        std::os::unix::fs::symlink(&target, &link).unwrap();
        write_text_atomic(&link, "new").unwrap();
        assert!(
            fs::symlink_metadata(&link)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert_eq!(fs::read_to_string(&target).unwrap(), "new");
    }
}
