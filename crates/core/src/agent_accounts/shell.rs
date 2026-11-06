//! The shell block: a small `claude()` function in `~/.zshrc` (or `~/.bash_profile`) between
//! markers, so a plain `claude` in a new terminal starts as the active account. An explicit
//! `CLAUDE_CONFIG_DIR=… claude` still wins. It replaces Claude Pool's block, which read
//! `~/.claude-pool/active`. Every change backs the file up first and is shown before it is made.
//!
//! Also the `cp1`…`cp9` shortcuts: links to `sawit`, each starting `claude` as that account.

use std::path::{Path, PathBuf};

use super::store::private_dir;
use crate::{CoreError, Result};

pub const BEGIN: &str = "# >>> sawit-accounts >>>";
pub const END: &str = "# <<< sawit-accounts <<<";
/// Claude Pool's markers: its block is replaced, never left beside Sawit's.
pub const POOL_BEGIN: &str = "# >>> claude-pool >>>";
pub const POOL_END: &str = "# <<< claude-pool <<<";

/// The block, its last newline included, reading the pointer in Sawit's data folder `root`.
pub fn snippet(root: &Path) -> String {
    let pointer = super::active_path(root, super::AgentKind::ClaudeCode);
    let pointer = crate::terminal::shell_quote(&pointer);
    format!(
        r#"{BEGIN}
# Managed by Sawit (Accounts page). Remove this block there, or by hand.
claude() {{
  if [[ -z "${{CLAUDE_CONFIG_DIR-}}" ]]; then
    local _sa_dir
    _sa_dir="$(cat {pointer} 2>/dev/null)"
    if [[ -n "$_sa_dir" && -d "$_sa_dir" ]]; then
      CLAUDE_CONFIG_DIR="$_sa_dir" command claude "$@"
      return
    fi
  fi
  command claude "$@"
}}
{END}
"#
    )
}

/// The startup file the login shell reads: `~/.zshrc` for zsh, `~/.bash_profile` for bash.
pub fn target(home: &Path, login_shell: &str) -> Option<PathBuf> {
    match Path::new(login_shell).file_name()?.to_str()? {
        "zsh" => Some(home.join(".zshrc")),
        "bash" => Some(home.join(".bash_profile")),
        _ => None,
    }
}

/// Where the block stands in a file's text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BlockState {
    Absent,
    Installed,
    /// Sawit's block, but not the current text.
    Outdated,
    /// A begin marker without its end: changed by hand, left alone.
    Unbalanced,
}

/// The span of a `begin`…`end` block, end-marker line and its newline included.
fn span(text: &str, begin: &str, end: &str) -> std::result::Result<Option<(usize, usize)>, ()> {
    let Some(start) = text.find(begin) else {
        return Ok(None);
    };
    let start = text[..start].rfind('\n').map(|i| i + 1).unwrap_or(0);
    let Some(e) = text[start..].find(end) else {
        return Err(());
    };
    let mut stop = start + e + end.len();
    if text[stop..].starts_with('\n') {
        stop += 1;
    }
    Ok(Some((start, stop)))
}

pub fn state(text: &str, root: &Path) -> BlockState {
    match span(text, BEGIN, END) {
        Err(()) => BlockState::Unbalanced,
        Ok(None) => BlockState::Absent,
        Ok(Some((a, b))) if text[a..b] == snippet(root) => BlockState::Installed,
        Ok(Some(_)) => BlockState::Outdated,
    }
}

/// Whether Claude Pool's block is in the text.
pub fn has_pool_block(text: &str) -> bool {
    text.contains(POOL_BEGIN)
}

/// The text with Sawit's block in place (updated where it is, else where Claude Pool's was, else
/// at the end) and Claude Pool's taken out.
pub fn with_block(text: &str, root: &Path) -> Result<String> {
    let unbalanced = |which: &str| {
        CoreError::Config(format!(
            "The startup file has a {which} start marker without its end marker. Fix it by hand, then try again."
        ))
    };
    let ours = span(text, BEGIN, END).map_err(|_| unbalanced("sawit-accounts"))?;
    let pool = span(text, POOL_BEGIN, POOL_END).map_err(|_| unbalanced("claude-pool"))?;
    let block = snippet(root);
    let mut out = text.to_string();
    match (ours, pool) {
        (Some((a, b)), Some((c, d))) => {
            // Take out the later one first, so the earlier span stays right.
            if a < c {
                out.replace_range(c..d, "");
                out.replace_range(a..b, &block);
            } else {
                out.replace_range(a..b, "");
                out.replace_range(c..d, &block);
            }
        }
        (Some((a, b)), None) => out.replace_range(a..b, &block),
        (None, Some((c, d))) => out.replace_range(c..d, &block),
        (None, None) => {
            if !out.is_empty() && !out.ends_with('\n') {
                out.push('\n');
            }
            if !out.is_empty() {
                out.push('\n');
            }
            out.push_str(&block);
        }
    }
    Ok(out)
}

/// The text without Sawit's block.
pub fn without_block(text: &str) -> Result<String> {
    match span(text, BEGIN, END) {
        Err(()) => Err(CoreError::Config(
            "The startup file has a sawit-accounts start marker without its end marker. Fix it by hand.".into(),
        )),
        Ok(None) => Ok(text.to_string()),
        Ok(Some((a, b))) => {
            let mut out = text.to_string();
            out.replace_range(a..b, "");
            Ok(out)
        }
    }
}

/// Writes `new` to the startup file `path` after backing it up into `accounts/backups/shell/`.
/// Returns the backup.
pub fn write(root: &Path, path: &Path, new: &str, now: i64) -> Result<Option<PathBuf>> {
    let backup = super::tracking::backup(root, "shell", path, now)?;
    crate::files::write_atomic(path, new.as_bytes())?;
    Ok(backup)
}

/// The number a `cp<n>` name stands for (1–9).
pub fn shortcut_number(program: &str) -> Option<usize> {
    let n = program.strip_prefix("cp")?;
    if n.len() != 1 {
        return None;
    }
    n.parse().ok().filter(|n| (1..=9).contains(n))
}

/// What a `cp<n>` link at `path` is now.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LinkState {
    Free,
    /// Already a link to this app.
    Ours,
    /// Claude Pool's link (to ClaudePool.app): replaced, its target noted in the backup.
    ClaudePool(PathBuf),
    /// Something else: never replaced.
    Other,
}

pub fn link_state(path: &Path, exe: &Path) -> LinkState {
    match std::fs::read_link(path) {
        Ok(t) if t == exe => LinkState::Ours,
        Ok(t) if t.to_string_lossy().contains("ClaudePool.app") => LinkState::ClaudePool(t),
        Ok(_) => LinkState::Other,
        Err(_) if path.symlink_metadata().is_ok() => LinkState::Other,
        Err(_) => LinkState::Free,
    }
}

/// Links `cp1`…`cp9` in `dir` to `exe`. Claude Pool's own links are replaced (their targets are
/// written to `accounts/backups/links-<time>.txt` first); anything else of that name stays.
/// Returns the names linked and those left alone.
pub fn install_shortcuts(
    root: &Path,
    dir: &Path,
    exe: &Path,
    now: i64,
) -> Result<(Vec<String>, Vec<String>)> {
    let (mut linked, mut skipped, mut replaced) = (Vec::new(), Vec::new(), Vec::new());
    let mut plan = Vec::new();
    for n in 1..=9 {
        let name = format!("cp{n}");
        let path = dir.join(&name);
        match link_state(&path, exe) {
            LinkState::Ours => linked.push(name),
            LinkState::Other => skipped.push(name),
            LinkState::ClaudePool(t) => {
                replaced.push(format!("{} -> {}", path.display(), t.display()));
                plan.push((name, path, true));
            }
            LinkState::Free => plan.push((name, path, false)),
        }
    }
    // Claude Pool's targets are written down before any link changes.
    if !replaced.is_empty() {
        let dir = super::store::dir(root).join("backups");
        private_dir(&dir)?;
        let stamp = crate::github::stamp(now).replace(':', "");
        std::fs::write(
            dir.join(format!("links-{stamp}.txt")),
            replaced.join("\n") + "\n",
        )?;
    }
    for (name, path, replace) in plan {
        if replace {
            std::fs::remove_file(&path)?;
        }
        std::os::unix::fs::symlink(exe, &path)?;
        linked.push(name);
    }
    linked.sort();
    Ok((linked, skipped))
}

/// Removes the `cp<n>` links in `dir` that point at `exe`; nothing else.
pub fn remove_shortcuts(dir: &Path, exe: &Path) -> Result<usize> {
    let mut n = 0;
    for i in 1..=9 {
        let path = dir.join(format!("cp{i}"));
        if link_state(&path, exe) == LinkState::Ours {
            std::fs::remove_file(&path)?;
            n += 1;
        }
    }
    Ok(n)
}

/// Where the shortcuts go: where Claude Pool's are (`/opt/homebrew/bin` here), else the first of
/// `~/.local/bin` and `/opt/homebrew/bin` on `path` that exists and can be written.
pub fn shortcut_dir(home: &Path, path: &str) -> Option<PathBuf> {
    let on_path: Vec<&str> = path.split(':').collect();
    [home.join(".local/bin"), PathBuf::from("/opt/homebrew/bin")]
        .into_iter()
        .find(|d| {
            on_path.contains(&d.to_string_lossy().as_ref())
                && d.is_dir()
                && std::fs::metadata(d).is_ok_and(|m| !m.permissions().readonly())
        })
}
