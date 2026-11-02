//! The skills Sawit ships ([`Method::Bundled`](super::Method::Bundled)): every file of each folder
//! in the repository's `skills/`, built into Sawit (crates/core/build.rs), written out into
//! Sawit's data folder for an install to copy into the agents' skills folders, and an agent's copy
//! compared with them.

use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

/// A file of a skill Sawit ships: its path in the skill's folder, and its bytes.
pub type File = (&'static str, &'static [u8]);

/// A skill Sawit ships: its name, and its files.
type Skill = (&'static str, &'static [File]);

// `SKILLS: &[Skill]`, made by the build from the repository's skills/ folder.
include!(concat!(env!("OUT_DIR"), "/bundled_skills.rs"));

/// The names of the skills Sawit ships.
pub fn names() -> impl Iterator<Item = &'static str> {
    SKILLS.iter().map(|(name, _)| *name)
}

/// The files of the skill `name`, by their path in its folder (`SKILL.md`, `references/…`).
pub fn files(name: &str) -> Option<&'static [File]> {
    SKILLS
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, files)| *files)
}

/// The text of the file `path` of the skill `name`: its `SKILL.md`, say.
pub fn text(name: &str, path: &str) -> Option<&'static str> {
    let (_, bytes) = files(name)?.iter().find(|(p, _)| *p == path)?;
    std::str::from_utf8(bytes).ok()
}

/// The `name` a SKILL.md's frontmatter gives its skill, unquoted.
pub fn skill_name(text: &str) -> Option<&str> {
    let rest = text.strip_prefix("---")?.trim_start_matches([' ', '\t']);
    let rest = rest
        .strip_prefix("\r\n")
        .or_else(|| rest.strip_prefix('\n'))?;
    let front = rest
        .lines()
        .take_while(|line| !matches!(line.trim_end(), "---" | "..."));
    let value = front
        .filter_map(|line| line.strip_prefix("name:"))
        .map(str::trim)
        .next()?;
    let unquoted = ['"', '\'']
        .into_iter()
        .find_map(|q| value.strip_prefix(q).and_then(|v| v.strip_suffix(q)));
    Some(unquoted.unwrap_or(value))
}

/// What an agent's skills folder holds where the skill `name` Sawit ships goes, beside this
/// Sawit's version of it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Held {
    /// This Sawit's version, file for file.
    Same,
    /// Sawit's skill (its SKILL.md names it), but not as this Sawit ships it: edited there, or
    /// another Sawit's version.
    Differs,
    /// Not Sawit's: a SKILL.md that names another skill, or none. Sawit never replaces or removes
    /// it.
    Other,
}

/// What `folder`, an agent's `<skills folder>/<name>`, holds beside the skill `name` Sawit ships
/// (a link to a folder followed): `None` when nothing is there, or a link to nothing.
pub fn held(name: &str, folder: &Path) -> Option<Held> {
    folder.metadata().ok()?;
    let files = files(name)?;
    let skill = std::fs::read_to_string(folder.join("SKILL.md")).ok();
    if skill.as_deref().and_then(skill_name) != Some(name) {
        return Some(Held::Other);
    }
    let mut seen = 0;
    let same = holds_only(folder, "", files, &mut seen) && seen == files.len();
    Some(if same { Held::Same } else { Held::Differs })
}

/// Whether `folder`, the skill folder's `rel`, holds only files of `files` with their bytes, and
/// the folders they are in (Finder's `.DS_Store` aside), counting them into `seen`. A link inside
/// isn't what a copy of Sawit's has.
fn holds_only(folder: &Path, rel: &str, files: &[File], seen: &mut usize) -> bool {
    let Ok(entries) = std::fs::read_dir(folder) else {
        return false;
    };
    for entry in entries {
        let Ok(entry) = entry else { return false };
        let (Some(name), Ok(kind)) = (
            entry.file_name().to_str().map(String::from),
            entry.file_type(),
        ) else {
            return false;
        };
        if name == ".DS_Store" {
            continue;
        }
        let at = if rel.is_empty() {
            name
        } else {
            format!("{rel}/{name}")
        };
        let inside = format!("{at}/");
        let fits = if kind.is_dir() {
            files.iter().any(|(file, _)| file.starts_with(&inside))
                && holds_only(&entry.path(), &at, files, seen)
        } else {
            let shipped = files.iter().find(|(file, _)| *file == at);
            kind.is_file()
                && shipped.is_some_and(|(_, bytes)| {
                    std::fs::read(entry.path()).is_ok_and(|b| b == *bytes)
                })
        };
        if !fits {
            return false;
        }
        *seen += usize::from(!kind.is_dir());
    }
    true
}

/// Writes the skill `name` out into `root`, as `<root>/<name>/`, the way this Sawit ships it: a
/// file is written only when what is there differs, and whatever else that folder holds is
/// removed, so a copy of the folder is this Sawit's version. Returns the folder.
pub fn write_out(name: &str, root: &Path) -> io::Result<PathBuf> {
    let files = files(name).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::NotFound,
            format!("Sawit ships no skill named {name}."),
        )
    })?;
    write_files(name, files, root)
}

/// [`write_out`] of the skill `name` whose files are `files`.
pub(super) fn write_files(name: &str, files: &[File], root: &Path) -> io::Result<PathBuf> {
    let dir = root.join(name);
    // A link in its place would have the writes and removals land elsewhere.
    if dir.symlink_metadata().is_ok_and(|m| !m.is_dir()) {
        std::fs::remove_file(&dir)?;
    }
    std::fs::create_dir_all(&dir)?;
    for (path, bytes) in files {
        let to = dir.join(path);
        // Unchanged, and in the folder itself: a file reached through a link to a folder is
        // elsewhere, and the link goes (prune), so it's written into a folder of its own.
        let file = to.symlink_metadata().is_ok_and(|m| m.is_file()) && real_folders(&dir, &to);
        if file && std::fs::read(&to).is_ok_and(|there| there == *bytes) {
            continue;
        }
        make_folders(&dir, &to)?;
        if to.symlink_metadata().is_ok_and(|m| m.is_dir()) {
            std::fs::remove_dir_all(&to)?;
        }
        write_whole(root, name, &to, bytes)?;
    }
    prune(&dir, "", files)?;
    Ok(dir)
}

/// Writes `bytes` to `to` in the skill `name`'s folder in `root`: first to a file of its own
/// beside that folder, then moved over `to`, so a copy of the folder made meanwhile never reads
/// half a file, and two Sawits writing it out at once never remove each other's.
fn write_whole(root: &Path, name: &str, to: &Path, bytes: &[u8]) -> io::Result<()> {
    static WRITES: AtomicUsize = AtomicUsize::new(0);
    let n = WRITES.fetch_add(1, Ordering::Relaxed);
    let part = root.join(format!(".{name}.part-{}-{n}", std::process::id()));
    std::fs::write(&part, bytes)
        .and_then(|()| std::fs::rename(&part, to))
        .inspect_err(|_| {
            let _ = std::fs::remove_file(&part);
        })
}

/// Whether every folder between the skill's folder `dir` and the file `to` is a folder, not a link.
fn real_folders(dir: &Path, to: &Path) -> bool {
    let parent = to.parent().unwrap_or(dir);
    parent
        .ancestors()
        .take_while(|folder| *folder != dir && folder.starts_with(dir))
        .all(|folder| folder.symlink_metadata().is_ok_and(|m| m.is_dir()))
}

/// Makes the folders between the skill's folder `dir` and the file `to`, a file or link in the
/// way of one taken out first.
fn make_folders(dir: &Path, to: &Path) -> io::Result<()> {
    let parent = to.parent().unwrap_or(dir);
    for folder in parent
        .ancestors()
        .take_while(|folder| folder.starts_with(dir))
    {
        if folder.symlink_metadata().is_ok_and(|m| !m.is_dir()) {
            std::fs::remove_file(folder)?;
        }
    }
    std::fs::create_dir_all(parent)
}

/// Removes from `folder`, the skill's folder `rel`, what isn't one of `files` or a folder of them.
fn prune(folder: &Path, rel: &str, files: &[File]) -> io::Result<()> {
    for entry in std::fs::read_dir(folder)? {
        let entry = entry?;
        let path = entry.path();
        // Not followed: a link is removed, never what it points at.
        let kind = entry.file_type()?;
        let at = entry.file_name().to_str().map(|name| match rel {
            "" => name.to_string(),
            _ => format!("{rel}/{name}"),
        });
        let Some(at) = at else {
            std::fs::remove_file(&path).or_else(|_| std::fs::remove_dir_all(&path))?;
            continue;
        };
        let inside = format!("{at}/");
        if kind.is_dir() && files.iter().any(|(file, _)| file.starts_with(&inside)) {
            prune(&path, &at, files)?;
        } else if kind.is_dir() {
            std::fs::remove_dir_all(&path)?;
        } else if !(kind.is_file() && files.iter().any(|(file, _)| *file == at)) {
            std::fs::remove_file(&path)?;
        }
    }
    Ok(())
}
