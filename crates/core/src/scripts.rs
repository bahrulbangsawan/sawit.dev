//! Run panel: the scripts a project declares (`package.json` scripts with
//! the right package manager, workspaces included, plus deno tasks, Make
//! targets, composer scripts and cargo) and the user's own saved commands,
//! one or more command lines per entry. Saved commands live in one JSON
//! file, keyed by project folder.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::files::write_text_atomic;
use crate::{CoreError, Result};

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

fn read_json(path: &Path) -> Option<Value> {
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
fn workspace_patterns(root: &Path, manifest: &Value) -> Vec<String> {
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

/// How the command lines of a saved command run.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RunMode {
    /// One after another in one terminal, stopping at the first failure.
    #[default]
    Sequence,
    /// All at once, each in its own terminal.
    Parallel,
}

/// A command the user saved for a project: one or more command lines.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SavedCommand {
    pub id: String,
    pub name: String,
    pub commands: Vec<String>,
    #[serde(default)]
    pub mode: RunMode,
}

impl SavedCommand {
    pub fn new(name: &str, commands: Vec<String>, mode: RunMode) -> SavedCommand {
        SavedCommand {
            id: new_id(),
            name: name.trim().to_string(),
            commands: clean_commands(commands),
            mode,
        }
    }

    /// What each terminal types: all lines joined with `&&` in sequence,
    /// one terminal per line in parallel.
    pub fn lines(&self) -> Vec<String> {
        match self.mode {
            RunMode::Sequence if !self.commands.is_empty() => vec![self.commands.join(" && ")],
            RunMode::Sequence => Vec::new(),
            RunMode::Parallel => self.commands.clone(),
        }
    }
}

/// Trimmed, without empty lines.
pub fn clean_commands(commands: Vec<String>) -> Vec<String> {
    commands
        .into_iter()
        .map(|c| c.trim().to_string())
        .filter(|c| !c.is_empty())
        .collect()
}

fn new_id() -> String {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or_default();
    format!("{nanos:x}-{:x}", NEXT.fetch_add(1, Ordering::Relaxed))
}

/// Every project's saved commands.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SavedCommands {
    /// By project folder (absolute path).
    pub projects: BTreeMap<String, Vec<SavedCommand>>,
}

impl SavedCommands {
    /// A missing file is empty; a corrupt one is an error (so it is never
    /// overwritten with nothing).
    pub fn load(path: &Path) -> Result<SavedCommands> {
        match std::fs::read_to_string(path) {
            Ok(text) => serde_json::from_str(&text)
                .map_err(|e| CoreError::Parse(format!("{}: {e}", path.display()))),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(SavedCommands::default()),
            Err(e) => Err(e.into()),
        }
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let text =
            serde_json::to_string_pretty(self).map_err(|e| CoreError::Parse(e.to_string()))?;
        write_text_atomic(path, &format!("{text}\n"))
    }

    fn key(root: &Path) -> String {
        root.to_string_lossy().into_owned()
    }

    pub fn for_project(&self, root: &Path) -> &[SavedCommand] {
        self.projects
            .get(&Self::key(root))
            .map(Vec::as_slice)
            .unwrap_or_default()
    }

    pub fn get(&self, root: &Path, id: &str) -> Option<&SavedCommand> {
        self.for_project(root).iter().find(|c| c.id == id)
    }

    /// Adds `command`, or replaces the one with its id.
    pub fn upsert(&mut self, root: &Path, command: SavedCommand) {
        let list = self.projects.entry(Self::key(root)).or_default();
        match list.iter_mut().find(|c| c.id == command.id) {
            Some(existing) => *existing = command,
            None => list.push(command),
        }
    }

    pub fn remove(&mut self, root: &Path, id: &str) -> bool {
        let key = Self::key(root);
        let Some(list) = self.projects.get_mut(&key) else {
            return false;
        };
        let before = list.len();
        list.retain(|c| c.id != id);
        let removed = list.len() != before;
        if list.is_empty() {
            self.projects.remove(&key);
        }
        removed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(path: &Path, text: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    #[test]
    fn package_scripts_use_the_lockfiles_tool_in_file_order() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write(
            &root.join("package.json"),
            r#"{"name":"web","scripts":{"dev":"vite","build":"vite build","lint:fix":"eslint --fix ."}}"#,
        );
        write(&root.join("pnpm-lock.yaml"), "");
        let groups = scan(root);
        assert_eq!(groups.len(), 1);
        let g = &groups[0];
        assert_eq!(
            (g.label.as_str(), g.tool.as_str(), g.rel.as_str()),
            ("web", "pnpm", "")
        );
        let commands: Vec<_> = g.scripts.iter().map(|s| s.command.as_str()).collect();
        assert_eq!(
            commands,
            ["pnpm run dev", "pnpm run build", "pnpm run lint:fix"]
        );
        assert_eq!(g.scripts[0].body, "vite");
    }

    #[test]
    fn package_manager_field_wins_and_npm_is_the_fallback() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write(&root.join("yarn.lock"), "");
        let m: Value = serde_json::from_str(r#"{"packageManager":"bun@1.2.0"}"#).unwrap();
        assert_eq!(package_manager(root, root, &m), "bun");
        assert_eq!(package_manager(root, root, &Value::Null), "yarn");
        let other = tempfile::tempdir().unwrap();
        assert_eq!(
            package_manager(other.path(), other.path(), &Value::Null),
            "npm"
        );
    }

    #[test]
    fn workspaces_are_scanned_with_the_root_lockfile() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write(
            &root.join("package.json"),
            r#"{"name":"mono","workspaces":["apps/*","!apps/legacy"],"scripts":{"dev":"turbo dev"}}"#,
        );
        write(&root.join("bun.lock"), "");
        write(
            &root.join("apps/web/package.json"),
            r#"{"name":"@m/web","scripts":{"dev":"next dev"}}"#,
        );
        write(
            &root.join("apps/api/package.json"),
            r#"{"scripts":{"start":"node ."}}"#,
        );
        write(
            &root.join("apps/legacy/package.json"),
            r#"{"scripts":{"x":"y"}}"#,
        );
        write(&root.join("apps/notes/readme.md"), "");
        let groups = scan(root);
        let labels: Vec<_> = groups.iter().map(|g| g.label.as_str()).collect();
        assert_eq!(labels, ["mono", "apps/api", "@m/web"]);
        assert_eq!(groups[2].scripts[0].command, "bun run dev");
        assert_eq!(groups[2].rel, "apps/web");
    }

    #[test]
    fn pnpm_workspace_yaml_lists_packages() {
        let yaml = "packages:\n  - 'apps/*'\n  - \"tools/cli\" # cli\n  - '!**/test/**'\ncatalog:\n  - nope\n";
        assert_eq!(pnpm_packages(yaml), ["apps/*", "tools/cli", "!**/test/**"]);
    }

    #[test]
    fn make_targets_skip_specials_patterns_and_variables() {
        let text = ".PHONY: dev\nCC := gcc\nX = 1\ndev: deps\n\tgo run .\nbuild test: \n%.o: %.c\n\t$(CC)\n# lint:\n";
        assert_eq!(make_targets(text), ["dev", "build", "test"]);
    }

    #[test]
    fn deno_jsonc_tasks_and_cargo() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write(
            &root.join("deno.jsonc"),
            "{\n // tasks\n \"tasks\": { \"start\": \"deno run -A main.ts\", \"x\": { \"command\": \"echo\" }, },\n}",
        );
        write(&root.join("Cargo.toml"), "[package]\n");
        let groups = scan(root);
        assert_eq!(groups[0].tool, "deno");
        assert_eq!(groups[0].scripts[0].command, "deno task start");
        assert_eq!(groups[0].scripts[1].body, "echo");
        assert_eq!(groups[1].scripts[0].command, "cargo run");
    }

    #[test]
    fn odd_script_names_are_quoted() {
        assert_eq!(run_command("npm", "dev:web"), "npm run dev:web");
        assert_eq!(run_command("npm", "it's on"), r"npm run 'it'\''s on'");
    }

    #[test]
    fn saved_command_lines_follow_the_mode() {
        let cmds = vec![
            " docker compose up -d ".into(),
            "".into(),
            "pnpm dev".into(),
        ];
        let mut c = SavedCommand::new(" Up ", cmds, RunMode::Sequence);
        assert_eq!(c.name, "Up");
        assert_eq!(c.lines(), ["docker compose up -d && pnpm dev"]);
        c.mode = RunMode::Parallel;
        assert_eq!(c.lines(), ["docker compose up -d", "pnpm dev"]);
    }

    #[test]
    fn saved_commands_round_trip_per_project() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("run-commands.json");
        let a = Path::new("/p/a");
        let mut saved = SavedCommands::default();
        let cmd = SavedCommand::new("dev", vec!["bun dev".into()], RunMode::Parallel);
        let id = cmd.id.clone();
        saved.upsert(a, cmd);
        saved.save(&file).unwrap();
        let loaded = SavedCommands::load(&file).unwrap();
        assert_eq!(loaded, saved);
        assert_eq!(loaded.for_project(a)[0].mode, RunMode::Parallel);
        assert!(loaded.for_project(Path::new("/p/b")).is_empty());
        let mut loaded = loaded;
        assert!(loaded.remove(a, &id));
        assert!(loaded.projects.is_empty());
        assert_eq!(
            SavedCommands::load(&dir.path().join("missing.json")).unwrap(),
            SavedCommands::default()
        );
    }
}
