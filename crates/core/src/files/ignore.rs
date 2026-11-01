//! Lines of ignore files (`.gitignore`, `info/exclude`): the line for a path, and adding it.

use std::path::Path;

use crate::Result;

/// The ignore-file line for `rel` (relative to the repository): anchored
/// to the root, with a trailing slash for folders. Spaces that end it are
/// escaped, as git drops them otherwise.
pub fn ignore_pattern(rel: &Path, is_dir: bool) -> String {
    let text = rel.to_string_lossy();
    let kept = text.trim_end_matches(' ');
    let mut line = String::from("/");
    for c in kept.chars() {
        if matches!(c, '*' | '?' | '[' | '!' | '#' | '\\') {
            line.push('\\');
        }
        line.push(c);
    }
    line.push_str(&"\\ ".repeat(text.len() - kept.len()));
    if is_dir {
        line.push('/');
    }
    line
}

/// Adds `line` to an ignore file (made if missing) unless it is already
/// listed; false when it was.
///
/// A link isn't written through: git doesn't read a `.gitignore` that is a
/// link, and a cloned repository could point one at a file of the user's
/// (`~/.zshrc`), which the line, a file name of the repository's choosing,
/// would be added to.
pub fn add_ignore_line(file: &Path, line: &str) -> Result<bool> {
    use std::io::Write as _;
    use std::os::unix::fs::OpenOptionsExt as _;

    let link = || {
        let name = file.file_name().unwrap_or_default().to_string_lossy();
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("“{name}” is a link, so Sawit won't write to it."),
        )
    };
    if std::fs::symlink_metadata(file).is_ok_and(|m| m.file_type().is_symlink()) {
        return Err(link().into());
    }
    let existing = match std::fs::read_to_string(file) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(e.into()),
    };
    if existing.lines().any(|l| l == line || l.trim() == line) {
        return Ok(false);
    }
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut out = std::fs::OpenOptions::new()
        .append(true)
        .create(true)
        // A link made since the check above.
        .custom_flags(libc::O_NOFOLLOW)
        .open(file)
        .map_err(|e| match e.raw_os_error() {
            Some(libc::ELOOP) => link(),
            _ => e,
        })?;
    let sep = if existing.is_empty() || existing.ends_with('\n') {
        ""
    } else {
        "\n"
    };
    writeln!(out, "{sep}{line}")?;
    Ok(true)
}
