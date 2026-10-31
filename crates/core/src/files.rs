//! Project files for the text editor: directory listing, safe reading
//! (size and encoding limits) and atomic saving.

use std::path::{Path, PathBuf};

use crate::Result;

/// Files larger than this open as a read-only notice instead of text.
pub const MAX_TEXT_BYTES: u64 = 5 * 1024 * 1024;

pub(crate) const HIDDEN: [&str; 6] = [
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
            read_only_reason: Some("This isn't a regular file (it's a pipe, socket or device), so it can't be opened as text.".into()),
        });
    }
    let size = meta.len();
    if size > MAX_TEXT_BYTES {
        return Ok(TextFile {
            text: String::new(),
            read_only_reason: Some(format!(
                "This file is {:.1} MB, over the editor's {} MB limit. Open it in another app.",
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
            read_only_reason: Some("Binary or non-UTF-8 file. Sawit can't show it as text.".into()),
        }),
    }
}

/// Writes via a private temp file in the same directory + rename, keeping
/// the original file's permissions. A symlink stays a symlink: its target is
/// what gets replaced ([`write_atomic`]).
pub fn write_text_atomic(path: &Path, text: &str) -> Result<()> {
    write_atomic(path, text.as_bytes())
}

/// Writes `bytes` to `path` so that a crash or power cut leaves the old file
/// or the new one, never part of either: to a private temp file in the same
/// directory ([`staging_file`]), synced to disk, then renamed over the file.
/// The file keeps its permissions (a new one gets 0644). A symlink stays a
/// symlink: its target is what gets replaced.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    use std::io::Write as _;
    use std::os::unix::fs::OpenOptionsExt as _;

    let path = match std::fs::symlink_metadata(path) {
        Ok(m) if m.file_type().is_symlink() => std::fs::canonicalize(path)?,
        _ => path.to_path_buf(),
    };
    let tmp = staging_file(&path);
    let result = (|| {
        // Owner-only until the final permissions are applied.
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&tmp)?;
        file.write_all(bytes)?;
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

/// Where [`write_atomic`] puts the bytes for the file at `path` (not a
/// symlink) before they replace it: `.<name>.sawit-tmp` beside it.
pub fn staging_file(path: &Path) -> PathBuf {
    let dir = path.parent().unwrap_or(Path::new("."));
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    dir.join(format!(".{name}.sawit-tmp"))
}

/// Makes an empty file or folder; fails if something is already there.
pub fn create(path: &Path, dir: bool) -> Result<()> {
    if dir {
        std::fs::create_dir(path)?;
    } else {
        std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)?;
    }
    Ok(())
}

/// Renames or moves `from` to `to`, refusing to replace anything there.
pub fn move_entry(from: &Path, to: &Path) -> Result<()> {
    if to.starts_with(from) && to != from {
        return Err(std::io::Error::other("A folder can't be moved into itself.").into());
    }
    if std::fs::symlink_metadata(to).is_ok() && !same_entry(from, to) {
        return Err(already_exists(to));
    }
    match std::fs::rename(from, to) {
        // Another volume: copy, then remove the original.
        Err(e) if e.raw_os_error() == Some(18) => {
            copy_entry(from, to)?;
            remove(from)
        }
        other => Ok(other?),
    }
}

/// Copies a file, or a folder and everything in it (links stay links).
/// Refuses to replace anything at `to`.
///
/// Only files, folders and links are copied. A folder's pipes, sockets and devices are left
/// out: copying a pipe opens it, which waits for a writer for ever, a socket can't be opened, and
/// a copy of either would be of no use. One of them on its own is refused. A folder copy that
/// fails part way is removed, so nothing half made is left that nothing could undo.
pub fn copy_entry(from: &Path, to: &Path) -> Result<()> {
    if to.starts_with(from) {
        return Err(std::io::Error::other("A folder can't be copied into itself.").into());
    }
    if std::fs::symlink_metadata(to).is_ok() {
        return Err(already_exists(to));
    }
    if !copied(&std::fs::symlink_metadata(from)?.file_type()) {
        let name = from.file_name().unwrap_or_default().to_string_lossy();
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("“{name}” is a pipe, socket or device, so it can't be copied."),
        )
        .into());
    }
    copy_tree(from, to)?;
    Ok(())
}

/// Whether [`copy_entry`] copies an entry of this type: a file, a folder or a link.
fn copied(kind: &std::fs::FileType) -> bool {
    kind.is_file() || kind.is_dir() || kind.is_symlink()
}

fn copy_tree(from: &Path, to: &Path) -> std::io::Result<()> {
    let meta = std::fs::symlink_metadata(from)?;
    let kind = meta.file_type();
    if kind.is_symlink() {
        std::os::unix::fs::symlink(std::fs::read_link(from)?, to)
    } else if kind.is_dir() {
        std::fs::create_dir(to)?;
        let filled = (|| {
            for entry in std::fs::read_dir(from)? {
                let entry = entry?;
                copy_tree(&entry.path(), &to.join(entry.file_name()))?;
            }
            std::fs::set_permissions(to, meta.permissions())
        })();
        if filled.is_err() {
            // Made just now, so it holds only what this copy put there.
            let _ = std::fs::remove_dir_all(to);
        }
        filled
    } else if kind.is_file() {
        std::fs::copy(from, to).map(|_| ())
    } else {
        // A pipe, socket or device in a folder: left out.
        Ok(())
    }
}

/// Deletes a file or a whole folder, for good.
pub fn remove(path: &Path) -> Result<()> {
    let meta = std::fs::symlink_metadata(path)?;
    if meta.is_dir() {
        std::fs::remove_dir_all(path)?;
    } else {
        std::fs::remove_file(path)?;
    }
    Ok(())
}

/// `path` with the links on its way resolved, as far as it exists: the longest part of it that
/// is there, as [`std::fs::canonicalize`] gives it, then the rest as written. Two names of one
/// place give one path this way, whether the file is there yet or not: on macOS
/// `/tmp/p/.nyawit/canvas.figcraft` and `/private/tmp/p/.nyawit/canvas.figcraft`, `/tmp` being a
/// link, before `.nyawit/` is made. A path none of which can be looked up stays as it is.
pub fn resolve(path: &Path) -> PathBuf {
    let mut rest = Vec::new();
    let mut there = path;
    loop {
        if let Ok(real) = there.canonicalize() {
            return rest.iter().rev().fold(real, |real, name| real.join(name));
        }
        match (there.parent(), there.file_name()) {
            (Some(parent), Some(name)) => {
                rest.push(name);
                there = parent;
            }
            _ => return path.to_path_buf(),
        }
    }
}

/// Whether `a` and `b` name one entry in two cases: names that differ only in case, in one
/// folder, for one file on disk. On a volume that ignores case (macOS's default) a rename between
/// them changes the case of the name. On one that tells case apart (a case-sensitive APFS volume,
/// a share), `README.md` and `readme.md` can be two files, and renaming one to the other would
/// replace it; then they aren't one entry.
fn same_entry(a: &Path, b: &Path) -> bool {
    a.parent() == b.parent()
        && a.to_string_lossy().to_lowercase() == b.to_string_lossy().to_lowercase()
        && same_file(a, b)
}

/// Whether `a` and `b` lead to one file, folder or link on disk (one device and inode; links
/// aren't followed). False when either can't be looked up.
fn same_file(a: &Path, b: &Path) -> bool {
    use std::os::unix::fs::MetadataExt as _;

    match (std::fs::symlink_metadata(a), std::fs::symlink_metadata(b)) {
        (Ok(a), Ok(b)) => (a.dev(), a.ino()) == (b.dev(), b.ino()),
        _ => false,
    }
}

fn already_exists(path: &Path) -> crate::CoreError {
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    std::io::Error::new(
        std::io::ErrorKind::AlreadyExists,
        format!("“{name}” already exists here."),
    )
    .into()
}

/// `dir/name` if nothing is there, else the first free "name copy",
/// "name copy 2", … (before the extension, for files).
pub fn free_name(dir: &Path, name: &str, is_dir: bool) -> PathBuf {
    let path = dir.join(name);
    if std::fs::symlink_metadata(&path).is_err() {
        return path;
    }
    let as_path = Path::new(name);
    let (stem, ext) = match (is_dir, as_path.file_stem(), as_path.extension()) {
        (false, Some(stem), Some(ext)) => (
            stem.to_string_lossy().into_owned(),
            format!(".{}", ext.to_string_lossy()),
        ),
        _ => (name.to_string(), String::new()),
    };
    (1..)
        .map(|n| match n {
            1 => dir.join(format!("{stem} copy{ext}")),
            n => dir.join(format!("{stem} copy {n}{ext}")),
        })
        .find(|p| std::fs::symlink_metadata(p).is_err())
        .expect("some name is free")
}

/// The ignore-file line for `rel` (relative to the repository): anchored
/// to the root, with a trailing slash for folders.
pub fn ignore_pattern(rel: &Path, is_dir: bool) -> String {
    let mut line = String::from("/");
    for c in rel.to_string_lossy().chars() {
        if matches!(c, '*' | '?' | '[' | '!' | '#' | '\\') {
            line.push('\\');
        }
        line.push(c);
    }
    if is_dir {
        line.push('/');
    }
    line
}

/// Adds `line` to an ignore file (made if missing) unless it is already
/// listed; false when it was.
pub fn add_ignore_line(file: &Path, line: &str) -> Result<bool> {
    use std::io::Write as _;

    let existing = match std::fs::read_to_string(file) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(e.into()),
    };
    if existing.lines().any(|l| l.trim() == line) {
        return Ok(false);
    }
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut out = std::fs::OpenOptions::new()
        .append(true)
        .create(true)
        .open(file)?;
    let sep = if existing.is_empty() || existing.ends_with('\n') {
        ""
    } else {
        "\n"
    };
    writeln!(out, "{sep}{line}")?;
    Ok(true)
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
    /// Mermaid source (`.mmd`), previewed as the diagram.
    Mermaid,
    /// Viewers.
    Image,
    Pdf,
    Video,
    /// A FigCraft document (`.figcraft`), shown on a canvas bound to the file.
    Figcraft,
    /// An Excalidraw scene (`.excalidraw`), imported onto a canvas that is saved as the
    /// `.figcraft` file beside it.
    Excalidraw,
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
        "mmd" | "mermaid" => FileKind::Mermaid,
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" | "ico" | "tif" | "tiff" | "heic"
        | "heif" | "avif" => FileKind::Image,
        "pdf" => FileKind::Pdf,
        "mp4" | "m4v" | "mov" | "webm" | "mkv" | "avi" => FileKind::Video,
        "figcraft" => FileKind::Figcraft,
        "excalidraw" => FileKind::Excalidraw,
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
        ".bashrc" | ".zshrc" | ".profile" | ".bash_profile" | ".zprofile" | ".zshenv"
        | "bashrc" | "bash.bashrc" | "zshrc" | "zshenv" | "zprofile" | "profile" => {
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
        "text" => "Text",
        "sql" => "SQL",
        "typescript" => "TypeScript",
        "tsx" => "TSX",
        "javascript" => "JavaScript",
        "json" => "JSON",
        "markdown" => "Markdown",
        "mmd" | "mermaid" => "Mermaid",
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
    fn resolve_gives_one_path_for_two_names_of_a_place_there_or_not() {
        let d = tempfile::tempdir().unwrap();
        let real = d.path().join("project");
        fs::create_dir(&real).unwrap();
        let link = d.path().join("link");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        let canonical = real.canonicalize().unwrap();
        // A file that is there, one that isn't yet, and one in a folder that isn't yet.
        fs::write(real.join("a.figcraft"), "").unwrap();
        for name in ["a.figcraft", "new.figcraft", ".nyawit/canvas.figcraft"] {
            assert_eq!(resolve(&link.join(name)), canonical.join(name), "{name}");
            assert_eq!(resolve(&real.join(name)), canonical.join(name), "{name}");
        }
        // A path with nothing of it there stays as it is.
        assert_eq!(
            resolve(Path::new("relative/x")),
            PathBuf::from("relative/x")
        );
    }

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
    fn free_name_counts_copies_before_the_extension() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path();
        assert_eq!(free_name(p, "a.rs", false), p.join("a.rs"));
        fs::write(p.join("a.rs"), "").unwrap();
        assert_eq!(free_name(p, "a.rs", false), p.join("a copy.rs"));
        fs::write(p.join("a copy.rs"), "").unwrap();
        assert_eq!(free_name(p, "a.rs", false), p.join("a copy 2.rs"));
        fs::write(p.join(".env"), "").unwrap();
        assert_eq!(free_name(p, ".env", false), p.join(".env copy"));
        fs::create_dir(p.join("v1.2")).unwrap();
        assert_eq!(free_name(p, "v1.2", true), p.join("v1.2 copy"));
    }

    #[test]
    fn copies_and_moves_folders_without_replacing() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path();
        fs::create_dir_all(p.join("src/inner")).unwrap();
        fs::write(p.join("src/inner/x.txt"), "x").unwrap();
        copy_entry(&p.join("src"), &p.join("dup")).unwrap();
        assert_eq!(fs::read_to_string(p.join("dup/inner/x.txt")).unwrap(), "x");
        assert!(copy_entry(&p.join("src"), &p.join("dup")).is_err());
        assert!(copy_entry(&p.join("src"), &p.join("src/inner/again")).is_err());
        assert!(move_entry(&p.join("dup"), &p.join("src")).is_err());
        move_entry(&p.join("dup"), &p.join("moved")).unwrap();
        assert!(p.join("moved/inner/x.txt").exists() && !p.join("dup").exists());
        remove(&p.join("moved")).unwrap();
        assert!(!p.join("moved").exists());
    }

    /// Duplicate and Paste copy a folder's files, links and folders, and leave out the pipes and
    /// sockets in it (FILES-12). Opening a pipe with no writer, to copy it, waited for ever; a
    /// socket can't be opened, which failed the copy halfway and left a partial copy. A pipe or
    /// socket on its own isn't copied: nothing is made, and the copy says why.
    #[test]
    fn copying_a_folder_leaves_its_pipes_and_sockets_out() {
        use std::os::unix::fs::OpenOptionsExt as _;

        let d = tempfile::tempdir().unwrap();
        let src = d.path().join("src");
        fs::create_dir_all(src.join("run")).unwrap();
        fs::write(src.join("a.txt"), "a").unwrap();
        std::os::unix::fs::symlink("a.txt", src.join("link")).unwrap();
        let pipe = src.join("run/pipe");
        assert!(
            std::process::Command::new("mkfifo")
                .arg(&pipe)
                .status()
                .unwrap()
                .success()
        );
        let _socket = std::os::unix::net::UnixListener::bind(src.join("run/app.sock")).unwrap();

        let copy = d.path().join("copy");
        let (tx, rx) = std::sync::mpsc::channel();
        let (from, to) = (src.clone(), copy.clone());
        std::thread::spawn(move || tx.send(copy_entry(&from, &to).map_err(|e| e.to_string())));
        let copied = rx.recv_timeout(std::time::Duration::from_secs(5));
        // A copy that opened the pipe waits for a writer: this one lets it go.
        let _ = fs::OpenOptions::new()
            .write(true)
            .custom_flags(libc::O_NONBLOCK)
            .open(&pipe);
        copied.expect("the copy waited on the pipe").unwrap();
        assert_eq!(fs::read_to_string(copy.join("a.txt")).unwrap(), "a");
        assert!(
            fs::symlink_metadata(copy.join("link"))
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert!(copy.join("run").is_dir());
        assert!(fs::symlink_metadata(copy.join("run/pipe")).is_err());
        assert!(fs::symlink_metadata(copy.join("run/app.sock")).is_err());

        for name in ["pipe", "app.sock"] {
            let to = d.path().join(format!("{name} copy"));
            let refused = copy_entry(&src.join("run").join(name), &to).unwrap_err();
            assert!(refused.to_string().contains("can't be copied"), "{refused}");
            assert!(fs::symlink_metadata(&to).is_err(), "{name}");
        }
    }

    /// A folder copy that fails part way, here on a file it may not read, leaves nothing behind:
    /// no half-made copy that nothing records and nothing can undo.
    #[test]
    fn a_folder_copy_that_fails_leaves_nothing_behind() {
        use std::os::unix::fs::PermissionsExt as _;

        let d = tempfile::tempdir().unwrap();
        let src = d.path().join("src");
        fs::create_dir_all(src.join("inner")).unwrap();
        fs::write(src.join("inner/a.txt"), "a").unwrap();
        fs::write(src.join("inner/secret"), "s").unwrap();
        fs::set_permissions(src.join("inner/secret"), fs::Permissions::from_mode(0o000)).unwrap();
        let copy = d.path().join("copy");
        assert!(copy_entry(&src, &copy).is_err());
        assert!(fs::symlink_metadata(&copy).is_err());
        // Let the temp folder go.
        fs::set_permissions(src.join("inner/secret"), fs::Permissions::from_mode(0o600)).unwrap();
    }

    /// A rename that changes only the case of a name goes ahead (the volume holding the temp
    /// folder ignores case, as macOS's does by default: one file has both names).
    #[test]
    fn a_rename_that_changes_only_the_case_goes_ahead() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path();
        fs::write(p.join("README.md"), "text").unwrap();
        move_entry(&p.join("README.md"), &p.join("readme.md")).unwrap();
        let names: Vec<_> = list_dir(p).unwrap().into_iter().map(|e| e.name).collect();
        assert_eq!(names, ["readme.md"]);
        assert_eq!(fs::read_to_string(p.join("readme.md")).unwrap(), "text");
    }

    /// Two names are one file on disk only when they lead to one inode: two files never are, two
    /// hard links to one file are, and a name with nothing there is no file.
    #[test]
    fn same_file_is_one_inode() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path();
        fs::write(p.join("a.txt"), "a").unwrap();
        fs::write(p.join("b.txt"), "b").unwrap();
        fs::hard_link(p.join("a.txt"), p.join("link.txt")).unwrap();
        assert!(same_file(&p.join("a.txt"), &p.join("a.txt")));
        assert!(!same_file(&p.join("a.txt"), &p.join("b.txt")));
        assert!(same_file(&p.join("a.txt"), &p.join("link.txt")));
        assert!(!same_file(&p.join("a.txt"), &p.join("missing.txt")));
        // Two names of one file that differ in more than case are still two entries: the
        // rename is refused rather than leaving both names as they were.
        assert!(move_entry(&p.join("a.txt"), &p.join("link.txt")).is_err());
        assert!(p.join("a.txt").exists() && p.join("link.txt").exists());
    }

    /// On a volume that tells case apart, `README.md` and `readme.md` are two files, and renaming
    /// one to the other's name replaced it for good (FILES-4): the names matched but for case,
    /// and that was taken for a rename of the case alone. Now it is refused, and both are kept;
    /// a rename to a case of the name that nothing has still goes ahead.
    ///
    /// Makes a sparse case-sensitive APFS disk image (about 8 MB) in a temp folder and mounts it
    /// there (`hdiutil`, no admin password):
    /// `cargo test -p sawit-core -- --ignored case_sensitive`.
    #[test]
    #[ignore = "makes and mounts a disk image"]
    fn renaming_to_another_files_name_in_other_case_replaces_nothing_on_a_case_sensitive_volume() {
        struct Mounted(PathBuf);
        impl Drop for Mounted {
            fn drop(&mut self) {
                let _ = std::process::Command::new("hdiutil")
                    .args(["detach", "-force", "-quiet"])
                    .arg(&self.0)
                    .status();
            }
        }
        let hdiutil = |args: &[&std::ffi::OsStr]| {
            let status = std::process::Command::new("hdiutil")
                .args(args)
                .status()
                .unwrap();
            assert!(status.success(), "hdiutil {args:?}");
        };
        let d = tempfile::tempdir().unwrap();
        let image = d.path().join("case.sparseimage");
        let mount = d.path().join("volume");
        fs::create_dir(&mount).unwrap();
        hdiutil(&[
            "create".as_ref(),
            "-size".as_ref(),
            "40m".as_ref(),
            "-fs".as_ref(),
            "Case-sensitive APFS".as_ref(),
            "-volname".as_ref(),
            "sawit-case".as_ref(),
            "-type".as_ref(),
            "SPARSE".as_ref(),
            "-quiet".as_ref(),
            d.path().join("case").as_os_str(),
        ]);
        hdiutil(&[
            "attach".as_ref(),
            "-nobrowse".as_ref(),
            "-noverify".as_ref(),
            "-noautoopen".as_ref(),
            "-quiet".as_ref(),
            "-mountpoint".as_ref(),
            mount.as_os_str(),
            image.as_os_str(),
        ]);
        let _mounted = Mounted(mount.clone());
        let p = mount.join("project");
        fs::create_dir(&p).unwrap();
        fs::write(p.join("README.md"), "upper").unwrap();
        fs::write(p.join("readme.md"), "lower").unwrap();

        let refused = move_entry(&p.join("README.md"), &p.join("readme.md"));
        assert_eq!(fs::read_to_string(p.join("readme.md")).unwrap(), "lower");
        assert_eq!(fs::read_to_string(p.join("README.md")).unwrap(), "upper");
        assert!(refused.unwrap_err().to_string().contains("already exists"));

        move_entry(&p.join("README.md"), &p.join("Readme.md")).unwrap();
        let mut names: Vec<_> = list_dir(&p).unwrap().into_iter().map(|e| e.name).collect();
        names.sort();
        assert_eq!(names, ["Readme.md", "readme.md"]);
    }

    #[test]
    fn create_refuses_existing() {
        let d = tempfile::tempdir().unwrap();
        let f = d.path().join("new.txt");
        create(&f, false).unwrap();
        assert!(create(&f, false).is_err());
        create(&d.path().join("dir"), true).unwrap();
        assert!(d.path().join("dir").is_dir());
    }

    #[test]
    fn ignore_lines_are_anchored_and_added_once() {
        assert_eq!(ignore_pattern(Path::new("a/b.log"), false), "/a/b.log");
        assert_eq!(ignore_pattern(Path::new("dist"), true), "/dist/");
        assert_eq!(ignore_pattern(Path::new("#x*.md"), false), "/\\#x\\*.md");
        let d = tempfile::tempdir().unwrap();
        let f = d.path().join(".gitignore");
        fs::write(&f, "target").unwrap();
        assert!(add_ignore_line(&f, "/dist/").unwrap());
        assert!(!add_ignore_line(&f, "/dist/").unwrap());
        assert_eq!(fs::read_to_string(&f).unwrap(), "target\n/dist/\n");
        let ex = d.path().join(".git/info/exclude");
        assert!(add_ignore_line(&ex, "/a").unwrap());
        assert_eq!(fs::read_to_string(&ex).unwrap(), "/a\n");
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
        assert!(t.read_only_reason.unwrap().contains("Binary"));
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
    fn write_atomic_writes_bytes_through_a_staging_file() {
        use std::os::unix::fs::PermissionsExt;
        let d = tempfile::tempdir().unwrap();
        let f = d.path().join("board.figcraft");
        assert_eq!(staging_file(&f), d.path().join(".board.figcraft.sawit-tmp"));
        // Bytes that aren't text, to a new file, which gets the usual permissions.
        write_atomic(&f, &[0, 0xFF, 0xFE]).unwrap();
        assert_eq!(fs::read(&f).unwrap(), [0, 0xFF, 0xFE]);
        assert_eq!(
            fs::metadata(&f).unwrap().permissions().mode() & 0o777,
            0o644
        );
        // A folder where the staging file goes: the write fails, and the file is as it was.
        fs::create_dir(staging_file(&f)).unwrap();
        assert!(write_atomic(&f, b"new").is_err());
        assert_eq!(fs::read(&f).unwrap(), [0, 0xFF, 0xFE]);
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
        assert_eq!(language_label("text"), "Text");
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
            ("flow.mmd", Mermaid),
            ("flow.Mermaid", Mermaid),
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
    fn classify_figcraft_and_excalidraw() {
        use FileKind::*;
        for (file, kind) in [
            ("board.figcraft", Figcraft),
            ("/p/.nyawit/canvas.figcraft", Figcraft),
            ("Board.FIGCRAFT", Figcraft),
            ("sketch.excalidraw", Excalidraw),
            ("/p/.nyawit/canvas.excalidraw", Excalidraw),
            ("Sketch.Excalidraw", Excalidraw),
            // An Excalidraw library, and names without the extension, are something else.
            ("shapes.excalidrawlib", Text),
            ("figcraft", Text),
            (".excalidraw", Text),
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
