//! Hosts named in the user's `~/.ssh/config` and the files it includes.

use std::path::{Path, PathBuf};

use super::query_words;

/// A `Host` entry from `~/.ssh/config` (patterns left out: they can't be
/// connected to by name).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ConfigHost {
    pub alias: String,
    pub hostname: Option<String>,
    pub user: Option<String>,
    pub port: Option<u16>,
    pub jump: Option<String>,
}

impl ConfigHost {
    /// `user@hostname:port` as far as the config says.
    pub fn address(&self) -> String {
        let mut s = String::new();
        if let Some(user) = &self.user {
            s.push_str(user);
            s.push('@');
        }
        s.push_str(self.hostname.as_deref().unwrap_or(&self.alias));
        if let Some(port) = self.port.filter(|p| *p != 22) {
            s.push_str(&format!(":{port}"));
        }
        s
    }

    pub fn matches(&self, query: &str) -> bool {
        let hay = format!("{} {}", self.alias, self.address()).to_lowercase();
        query_words(query).iter().all(|w| hay.contains(w))
    }
}

/// The user's `~/.ssh/config`.
pub fn user_config_path() -> Option<PathBuf> {
    std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".ssh").join("config"))
}

/// Hosts named in `~/.ssh/config` and the files it `Include`s.
pub fn load_config_hosts() -> Vec<ConfigHost> {
    let Some(path) = user_config_path() else {
        return Vec::new();
    };
    let Ok(text) = std::fs::read_to_string(&path) else {
        return Vec::new();
    };
    let ssh_dir = path.parent().map(Path::to_path_buf).unwrap_or_default();
    let mut out = Vec::new();
    parse_config(
        &text,
        &ssh_dir,
        &mut |p| std::fs::read_to_string(p).ok(),
        0,
        &mut out,
    );
    out
}

/// Parses config `text`; `Include` paths are relative to `ssh_dir` and may
/// end in a `*` pattern. The first value of each keyword wins, as in `ssh`.
pub fn parse_config(
    text: &str,
    ssh_dir: &Path,
    read: &mut dyn FnMut(&Path) -> Option<String>,
    depth: usize,
    out: &mut Vec<ConfigHost>,
) {
    // Indices into `out` the current `Host` line declared.
    let mut current: Vec<usize> = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (key, value) = match line.split_once(|c: char| c.is_whitespace() || c == '=') {
            Some((k, v)) => (k.to_lowercase(), v.trim().trim_start_matches('=').trim()),
            None => continue,
        };
        let value = value.trim_matches('"');
        match key.as_str() {
            "host" => {
                current.clear();
                for alias in value.split_whitespace() {
                    let alias = alias.trim_matches('"');
                    if alias.contains(['*', '?', '!']) {
                        continue;
                    }
                    let ix = match out.iter().position(|h| h.alias == alias) {
                        Some(ix) => ix,
                        None => {
                            out.push(ConfigHost {
                                alias: alias.to_string(),
                                ..ConfigHost::default()
                            });
                            out.len() - 1
                        }
                    };
                    current.push(ix);
                }
            }
            // A `Match` block's settings apply conditionally; don't guess.
            "match" => current.clear(),
            "include" if depth < 8 => {
                for pattern in value.split_whitespace() {
                    for file in expand_include(pattern, ssh_dir) {
                        if let Some(text) = read(&file) {
                            parse_config(&text, ssh_dir, read, depth + 1, out);
                        }
                    }
                }
                // An included file ends its own blocks, not the includer's;
                // keep it simple and stop applying settings until the next `Host`.
                current.clear();
            }
            "hostname" => set_each(out, &current, |h| &mut h.hostname, value),
            "user" => set_each(out, &current, |h| &mut h.user, value),
            "proxyjump" => set_each(out, &current, |h| &mut h.jump, value),
            "port" => {
                if let Ok(port) = value.parse::<u16>() {
                    for &ix in &current {
                        out[ix].port.get_or_insert(port);
                    }
                }
            }
            _ => {}
        }
    }
}

fn set_each(
    out: &mut [ConfigHost],
    current: &[usize],
    field: impl Fn(&mut ConfigHost) -> &mut Option<String>,
    value: &str,
) {
    for &ix in current {
        field(&mut out[ix]).get_or_insert_with(|| value.to_string());
    }
}

/// `Include` targets: `~/` and relative paths resolved, a `*` in the file
/// name matched against the directory's entries (sorted, as `ssh` does).
fn expand_include(pattern: &str, ssh_dir: &Path) -> Vec<PathBuf> {
    let path = if let Some(rest) = pattern.strip_prefix("~/") {
        match std::env::var_os("HOME") {
            Some(home) => PathBuf::from(home).join(rest),
            None => return Vec::new(),
        }
    } else if Path::new(pattern).is_absolute() {
        PathBuf::from(pattern)
    } else {
        ssh_dir.join(pattern)
    };
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    if !name.contains('*') {
        return vec![path];
    }
    let Some(dir) = path.parent() else {
        return Vec::new();
    };
    let (prefix, suffix) = name.split_once('*').unwrap_or((&name, ""));
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.file_name().is_some_and(|n| {
                let n = n.to_string_lossy();
                n.len() >= prefix.len() + suffix.len()
                    && n.starts_with(prefix)
                    && n.ends_with(suffix)
            })
        })
        .collect();
    files.sort();
    files
}
