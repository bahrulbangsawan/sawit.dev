//! Run panel: the scripts a project declares (`package.json` scripts with
//! the right package manager, workspaces included, plus deno tasks, Make
//! targets, composer scripts and cargo) and the user's own saved commands,
//! one or more command lines per entry. Saved commands live in one JSON
//! file, keyed by project folder.

mod saved;
#[cfg(test)]
mod tests;

use std::path::{Path, PathBuf};

use serde_json::Value;

pub use saved::{RunMode, SavedCommand, SavedCommands, clean_commands};

/// At most this many workspace packages are scanned.
const MAX_PACKAGES: usize = 60;

/// One runnable script.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Script {
    /// Its name (`dev`, `build`).
    pub name: String,
    /// What to type into a shell: `pnpm run dev`.
    pub command: String,
    /// What it runs (`vite --port 3000`), when the file says.
    pub body: String,
}

/// The scripts of one file in one folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScriptGroup {
    /// The package name, else the folder relative to the project (`.` for it).
    pub label: String,
    /// Relative to the project root, `""` for the root itself.
    pub rel: String,
    pub dir: PathBuf,
    /// What runs them: `pnpm`, `bun`, `make`, `cargo`, …
    pub tool: String,
    /// The file they come from: `package.json`, `Makefile`, …
    pub source: String,
    pub scripts: Vec<Script>,
}

/// The package manager of the JS project in `dir`: the `packageManager`
/// field, else the lockfile in `dir` or a parent up to `root`, else npm.
pub fn package_manager(dir: &Path, root: &Path, manifest: &Value) -> &'static str {
    if let Some(pm) = manifest.get("packageManager").and_then(Value::as_str) {
        for tool in ["pnpm", "yarn", "bun", "npm"] {
            if pm.starts_with(tool) {
                return tool;
            }
        }
    }
    let mut at = Some(dir);
    while let Some(d) = at {
        for (lock, tool) in [
            ("bun.lock", "bun"),
            ("bun.lockb", "bun"),
            ("pnpm-lock.yaml", "pnpm"),
            ("yarn.lock", "yarn"),
            ("package-lock.json", "npm"),
        ] {
            if d.join(lock).is_file() {
                return tool;
            }
        }
        if d == root {
            break;
        }
        at = d.parent();
    }
    "npm"
}

/// `name` as one shell word.
pub fn shell_word(name: &str) -> String {
    let plain = !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_alphanumeric() || "/._-+,:@%=".contains(c));
    if plain {
        name.to_string()
    } else {
        format!("'{}'", name.replace('\'', r"'\''"))
    }
}

/// `tool run name` (`yarn` and `bun` take it as well).
pub fn run_command(tool: &str, name: &str) -> String {
    format!("{tool} run {}", shell_word(name))
}

pub(crate) fn read_json(path: &Path) -> Option<Value> {
    let text = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&text)
        .ok()
        .or_else(|| serde_json::from_str(&strip_jsonc(&text)).ok())
}

/// Drops `//` and `/* */` comments and trailing commas (outside strings).
fn strip_jsonc(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    let mut in_str = false;
    while let Some(c) = chars.next() {
        if in_str {
            out.push(c);
            if c == '\\' {
                if let Some(n) = chars.next() {
                    out.push(n);
                }
            } else if c == '"' {
                in_str = false;
            }
            continue;
        }
        match (c, chars.peek()) {
            ('"', _) => {
                in_str = true;
                out.push(c);
            }
            ('/', Some('/')) => {
                for n in chars.by_ref() {
                    if n == '\n' {
                        out.push('\n');
                        break;
                    }
                }
            }
            ('/', Some('*')) => {
                chars.next();
                let mut prev = ' ';
                for n in chars.by_ref() {
                    if prev == '*' && n == '/' {
                        break;
                    }
                    prev = n;
                }
            }
            (',', _) => {
                let rest: String = chars.clone().collect();
                if !rest.trim_start().starts_with(['}', ']']) {
                    out.push(c);
                }
            }
            _ => out.push(c),
        }
    }
    out
}

/// String entries of the object at `key`, in file order.
fn string_entries(value: &Value, key: &str) -> Vec<(String, String)> {
    value
        .get(key)
        .and_then(Value::as_object)
        .map(|o| {
            o.iter()
                .map(|(k, v)| {
                    let body = match v {
                        Value::String(s) => s.clone(),
                        Value::Array(a) => a
                            .iter()
                            .filter_map(Value::as_str)
                            .collect::<Vec<_>>()
                            .join(" && "),
                        // deno: { "command": "…" }
                        Value::Object(o) => o
                            .get("command")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_string(),
                        _ => String::new(),
                    };
                    (k.clone(), body)
                })
                .collect()
        })
        .unwrap_or_default()
}

fn rel_of(dir: &Path, root: &Path) -> String {
    dir.strip_prefix(root)
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn folder_label(rel: &str, root: &Path) -> String {
    if rel.is_empty() {
        root.file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| ".".into())
    } else {
        rel.to_string()
    }
}

/// The `package.json` scripts of `dir`.
fn js_group(dir: &Path, root: &Path) -> Option<(ScriptGroup, Value)> {
    let manifest = read_json(&dir.join("package.json"))?;
    let tool = package_manager(dir, root, &manifest);
    let rel = rel_of(dir, root);
    let scripts: Vec<Script> = string_entries(&manifest, "scripts")
        .into_iter()
        .map(|(name, body)| Script {
            command: run_command(tool, &name),
            name,
            body,
        })
        .collect();
    let label = manifest
        .get("name")
        .and_then(Value::as_str)
        .filter(|n| !n.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| folder_label(&rel, root));
    Some((
        ScriptGroup {
            label,
            rel,
            dir: dir.to_path_buf(),
            tool: tool.into(),
            source: "package.json".into(),
            scripts,
        },
        manifest,
    ))
}

/// Workspace globs from `package.json` (`workspaces`, or
/// `workspaces.packages`) and `pnpm-workspace.yaml`.
pub(crate) fn workspace_patterns(root: &Path, manifest: &Value) -> Vec<String> {
    let mut out = Vec::new();
    let list = manifest
        .get("workspaces")
        .and_then(|w| w.as_array().or_else(|| w.get("packages")?.as_array()));
    if let Some(list) = list {
        out.extend(list.iter().filter_map(Value::as_str).map(str::to_string));
    }
    if let Ok(yaml) = std::fs::read_to_string(root.join("pnpm-workspace.yaml")) {
        out.extend(pnpm_packages(&yaml));
    }
    out
}

/// The `packages:` list of a `pnpm-workspace.yaml`.
pub fn pnpm_packages(yaml: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut inside = false;
    for line in yaml.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if !line.starts_with([' ', '\t', '-']) {
            inside = trimmed.starts_with("packages:");
            continue;
        }
        if inside && let Some(item) = trimmed.strip_prefix('-') {
            let item = item.split(" #").next().unwrap_or_default().trim();
            let item = item.trim_matches(|c| c == '\'' || c == '"');
            if !item.is_empty() {
                out.push(item.to_string());
            }
        }
    }
    out
}

/// Folders matched by workspace `patterns` (`apps/*`, `packages/**`,
/// `tools/cli`); `!` exclusions are honored, deeper globs are not.
pub fn expand_workspaces(root: &Path, patterns: &[String]) -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = Vec::new();
    let excluded: Vec<PathBuf> = patterns
        .iter()
        .filter_map(|p| p.strip_prefix('!'))
        .map(|p| root.join(p.trim_start_matches("./").trim_end_matches('/')))
        .collect();
    for pattern in patterns.iter().filter(|p| !p.starts_with('!')) {
        let pattern = pattern.trim_start_matches("./").trim_end_matches('/');
        let (base, glob) = match pattern.find('*') {
            Some(ix) => (pattern[..ix].trim_end_matches('/'), true),
            None => (pattern, false),
        };
        let base = root.join(base);
        let mut found = Vec::new();
        if glob {
            if let Ok(entries) = std::fs::read_dir(&base) {
                found.extend(
                    entries
                        .flatten()
                        .map(|e| e.path())
                        .filter(|p| p.join("package.json").is_file()),
                );
            }
        } else if base.join("package.json").is_file() {
            found.push(base);
        }
        found.sort();
        for dir in found {
            if dir != root && !dirs.contains(&dir) && !excluded.contains(&dir) {
                dirs.push(dir);
            }
        }
    }
    dirs.truncate(MAX_PACKAGES);
    dirs
}

/// Targets of a Makefile that can be run on their own: not special
/// (`.PHONY`), not patterns (`%.o`), not variables.
pub fn make_targets(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for line in text.lines() {
        if line.starts_with(['\t', ' ', '#', '.']) {
            continue;
        }
        let Some((head, rest)) = line.split_once(':') else {
            continue;
        };
        // `X := 1`, `X ::= 1`
        if rest.starts_with('=') || rest.starts_with(":=") || head.contains('=') {
            continue;
        }
        for target in head.split_whitespace() {
            let ok = target
                .chars()
                .all(|c| c.is_alphanumeric() || "_-./".contains(c));
            if ok && !out.iter().any(|t| t == target) {
                out.push(target.to_string());
            }
        }
    }
    out
}

/// Everything runnable in the project at `root`: the root's own scripts
/// first, then workspace packages, then other tools.
pub fn scan(root: &Path) -> Vec<ScriptGroup> {
    let mut groups = Vec::new();
    if let Some((group, manifest)) = js_group(root, root) {
        let patterns = workspace_patterns(root, &manifest);
        groups.push(group);
        for dir in expand_workspaces(root, &patterns) {
            if let Some((group, _)) = js_group(&dir, root) {
                groups.push(group);
            }
        }
    }
    let label = folder_label("", root);
    let group = |tool: &str, source: &str, scripts: Vec<Script>| ScriptGroup {
        label: label.clone(),
        rel: String::new(),
        dir: root.to_path_buf(),
        tool: tool.into(),
        source: source.into(),
        scripts,
    };
    for file in ["deno.json", "deno.jsonc"] {
        if let Some(manifest) = read_json(&root.join(file)) {
            let tasks = string_entries(&manifest, "tasks")
                .into_iter()
                .map(|(name, body)| Script {
                    command: format!("deno task {}", shell_word(&name)),
                    name,
                    body,
                })
                .collect();
            groups.push(group("deno", file, tasks));
            break;
        }
    }
    if let Some(manifest) = read_json(&root.join("composer.json")) {
        let scripts = string_entries(&manifest, "scripts")
            .into_iter()
            .map(|(name, body)| Script {
                command: format!("composer run-script {}", shell_word(&name)),
                name,
                body,
            })
            .collect();
        groups.push(group("composer", "composer.json", scripts));
    }
    for file in ["Makefile", "makefile", "GNUmakefile"] {
        if let Ok(text) = std::fs::read_to_string(root.join(file)) {
            let targets = make_targets(&text)
                .into_iter()
                .map(|name| Script {
                    command: format!("make {}", shell_word(&name)),
                    name,
                    body: String::new(),
                })
                .collect();
            groups.push(group("make", file, targets));
            break;
        }
    }
    if root.join("Cargo.toml").is_file() {
        let scripts = ["run", "build", "test", "check"]
            .into_iter()
            .map(|name| Script {
                name: name.into(),
                command: format!("cargo {name}"),
                body: String::new(),
            })
            .collect();
        groups.push(group("cargo", "Cargo.toml", scripts));
    }
    groups.retain(|g| !g.scripts.is_empty());
    groups
}
