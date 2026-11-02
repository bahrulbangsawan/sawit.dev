//! A screen's files, as agents write them: the whole page (`html`), changes to it (`edits`, each a
//! `find` that is in the file once and its `replace`), or other files of the screen (`files`: CSS,
//! scripts, SVG, images as base64). A write is checked whole before anything is written, so one
//! that fails changes nothing. Each file is written beside its place and renamed into it, never
//! through a link: a cloned project's screens folder can hold one.

use std::path::{Path, PathBuf};

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use serde_json::Value;

use super::{INDEX, MAX_FILE, folder, read};

/// The most files one write may hold, and the deepest a file may sit in its screen's folder.
const MOST_FILES: usize = 64;
const DEEPEST: usize = 8;
/// The most files a screen's listing names.
const MOST_LISTED: usize = 500;

/// One write to a screen, as `canvas_screen_write` takes it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Write {
    /// The whole of `index.html`.
    pub html: Option<String>,
    /// Changes to the screen's files, in order.
    pub edits: Vec<Edit>,
    /// Other files, whole.
    pub files: Vec<File>,
}

/// A change to a file of the screen: `find`, which must be in it exactly once, becomes `replace`.
#[derive(Clone, Debug, PartialEq)]
pub struct Edit {
    /// The file, from the screen's folder; `index.html` when not given.
    pub path: String,
    pub find: String,
    pub replace: String,
}

/// A file of the screen, whole: text, or bytes given as base64.
#[derive(Clone, Debug, PartialEq)]
pub struct File {
    pub path: String,
    pub bytes: Vec<u8>,
}

impl Write {
    /// The write an op holds: its `html`, `edits` (`[{find, replace, path?}]`) and `files`
    /// (`[{path, content}]` or `[{path, base64}]`). It needs one of them; `html` and `edits` don't
    /// go together.
    pub fn from_op(op: &Value) -> Result<Write, String> {
        let html = match op.get("html") {
            None | Some(Value::Null) => None,
            Some(Value::String(html)) => Some(html.clone()),
            Some(_) => return Err("`html` must be the page's HTML, as text".into()),
        };
        let list = |key: &str| match op.get(key) {
            None | Some(Value::Null) => Ok(Vec::new()),
            Some(Value::Array(items)) => Ok(items.clone()),
            Some(_) => Err(format!("`{key}` must be a list")),
        };
        let edits = list("edits")?
            .iter()
            .map(|edit| {
                let text = |key: &str| edit.get(key).and_then(Value::as_str).map(str::to_string);
                let find = text("find").filter(|f| !f.is_empty());
                let find = find.ok_or("each edit needs `find`, the text it changes")?;
                let replace = text("replace").ok_or("each edit needs `replace`")?;
                let path = check_path(text("path").as_deref().unwrap_or(INDEX))?;
                Ok(Edit {
                    path,
                    find,
                    replace,
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        let files = list("files")?
            .iter()
            .map(|file| {
                let path = file.get("path").and_then(Value::as_str);
                let path = check_path(path.ok_or("each file needs its `path`")?)?;
                let bytes = match (file.get("content"), file.get("base64")) {
                    (Some(Value::String(text)), None) => text.as_bytes().to_vec(),
                    (None, Some(Value::String(data))) => STANDARD
                        .decode(data.trim())
                        .map_err(|e| format!("{path}: its base64 doesn't read: {e}"))?,
                    _ => return Err(format!("{path} needs `content` (text) or `base64`")),
                };
                Ok(File { path, bytes })
            })
            .collect::<Result<Vec<_>, String>>()?;
        if html.is_some() && !edits.is_empty() {
            return Err("give `html` (the whole page) or `edits`, not both".into());
        }
        if html.is_none() && edits.is_empty() && files.is_empty() {
            return Err("it needs `html`, `edits` or `files`".into());
        }
        if files.len() > MOST_FILES {
            return Err(format!("one write takes at most {MOST_FILES} files"));
        }
        Ok(Write { html, edits, files })
    }
}

/// `path` as a path in a screen's folder: relative, at most [`DEEPEST`] deep, with no `..`, `.`
/// or empty part, no `\` and no part that starts with `.`.
pub fn check_path(path: &str) -> Result<String, String> {
    let path = path.trim();
    let parts: Vec<&str> = path.split('/').collect();
    let bad = path.is_empty()
        || path.starts_with('/')
        || path.contains('\\')
        || path.contains('\0')
        || parts.len() > DEEPEST
        || parts.iter().any(|p| p.is_empty() || p.starts_with('.'));
    match bad {
        true => Err(format!(
            "“{path}” isn't a path in the screen's folder: give one like css/site.css"
        )),
        false => Ok(path.to_string()),
    }
}

/// Makes screen `name`'s folder in the screens folder `dir`, and a project's `.nyawit/.gitignore`
/// with it the first time. Its path.
pub fn make_folder(dir: &Path, name: &str) -> Result<PathBuf, String> {
    let made = folder(dir, name);
    let in_nyawit = dir.parent().filter(|parent| {
        parent
            .file_name()
            .is_some_and(|n| n == sawit_core::nyawit::DIR)
    });
    let result = match in_nyawit.and_then(Path::parent) {
        Some(project) => sawit_core::nyawit::ensure(project, &made),
        None => std::fs::create_dir_all(&made),
    };
    result.map_err(|e| format!("couldn't make {}: {e}", made.display()))?;
    no_links(dir, &made)?;
    Ok(made)
}

/// Writes `write` to the screen whose folder is `folder` (made if need be): the files it changes,
/// from the folder, in the order written. Every edit is checked against the files as they are
/// before anything is written.
pub fn apply(folder: &Path, write: &Write) -> Result<Vec<String>, String> {
    let mut out: Vec<(String, Vec<u8>)> = Vec::new();
    if let Some(html) = &write.html {
        out.push((INDEX.to_string(), html.as_bytes().to_vec()));
    }
    for file in &write.files {
        out.retain(|(path, _)| *path != file.path);
        out.push((file.path.clone(), file.bytes.clone()));
    }
    for edit in &write.edits {
        let current = match out.iter().position(|(path, _)| *path == edit.path) {
            Some(at) => out.remove(at).1,
            None => read(folder, &edit.path)
                .map_err(|e| format!("{}: couldn't read it to edit: {e}", edit.path))?,
        };
        let text = String::from_utf8(current)
            .map_err(|_| format!("{} isn't text, so it can't be edited", edit.path))?;
        let changed = replace_once(&text, &edit.find, &edit.replace)
            .map_err(|why| format!("{}: {why}", edit.path))?;
        out.push((edit.path.clone(), changed.into_bytes()));
    }
    if let Some((path, _)) = out.iter().find(|(_, bytes)| bytes.len() as u64 > MAX_FILE) {
        return Err(format!("{path} is over {} MiB", MAX_FILE / 1024 / 1024));
    }
    std::fs::create_dir_all(folder)
        .map_err(|e| format!("couldn't make {}: {e}", folder.display()))?;
    for (path, bytes) in &out {
        write_file(folder, path, bytes)?;
    }
    Ok(out.into_iter().map(|(path, _)| path).collect())
}

/// `text` with `find`, which must be in it exactly once, replaced by `replace`.
pub fn replace_once(text: &str, find: &str, replace: &str) -> Result<String, String> {
    let mut found = text.match_indices(find);
    let Some((at, _)) = found.next() else {
        return Err(format!("“{}” isn't in it", preview(find)));
    };
    if found.next().is_some() {
        return Err(format!(
            "“{}” is in it more than once: give more of the text around it",
            preview(find)
        ));
    }
    Ok(format!(
        "{}{replace}{}",
        &text[..at],
        &text[at + find.len()..]
    ))
}

/// The start of `text`, for an error.
fn preview(text: &str) -> String {
    let short: String = text.chars().take(60).collect();
    match short.len() < text.len() {
        true => format!("{short}…"),
        false => short,
    }
}

/// Writes `bytes` to `rel` in `folder`: to a file beside it, then renamed into place, so the page
/// never reads half a file and a link at that name is replaced, never written through. The
/// folders on the way are made, and none of them may be a link.
fn write_file(folder: &Path, rel: &str, bytes: &[u8]) -> Result<(), String> {
    let target = folder.join(rel);
    let parent = target.parent().unwrap_or(folder);
    std::fs::create_dir_all(parent).map_err(|e| format!("{rel}: couldn't make its folder: {e}"))?;
    no_links(folder, parent)?;
    let name = target.file_name().map(|n| n.to_string_lossy().into_owned());
    let temp = parent.join(format!(
        ".{}.{}.tmp",
        name.unwrap_or_default(),
        std::process::id()
    ));
    std::fs::write(&temp, bytes).map_err(|e| format!("{rel}: couldn't write it: {e}"))?;
    std::fs::rename(&temp, &target).map_err(|e| {
        let _ = std::fs::remove_file(&temp);
        format!("{rel}: couldn't write it: {e}")
    })
}

/// Refuses when a folder from `root` down to `path` is a link.
fn no_links(root: &Path, path: &Path) -> Result<(), String> {
    let mut at = root.to_path_buf();
    let rest = path.strip_prefix(root).unwrap_or(Path::new(""));
    let check = |at: &Path| match std::fs::symlink_metadata(at) {
        Ok(meta) if meta.file_type().is_symlink() => Err(format!(
            "{} is a link: a screen's files are never written through one",
            at.display()
        )),
        _ => Ok(()),
    };
    check(&at)?;
    for part in rest.components() {
        at.push(part);
        check(&at)?;
    }
    Ok(())
}

/// The files of the screen whose folder is `folder`, from the folder, sorted: links and what
/// starts with `.` left out, at most [`MOST_LISTED`].
pub fn list(folder: &Path) -> Vec<String> {
    let mut found = Vec::new();
    let mut stack = vec![(folder.to_path_buf(), String::new(), 0usize)];
    while let Some((dir, prefix, depth)) = stack.pop() {
        for entry in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            if name.starts_with('.') || kind.is_symlink() || found.len() >= MOST_LISTED {
                continue;
            }
            let rel = format!("{prefix}{name}");
            if kind.is_dir() && depth + 1 < DEEPEST {
                stack.push((entry.path(), format!("{rel}/"), depth + 1));
            } else if kind.is_file() {
                found.push(rel);
            }
        }
    }
    found.sort();
    found
}

/// The screen's page, `index.html`, as text: `None` while there is none.
pub fn page(folder: &Path) -> Option<String> {
    let bytes = read(folder, INDEX).ok()?;
    Some(String::from_utf8_lossy(&bytes).into_owned())
}
