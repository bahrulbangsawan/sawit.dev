//! Claude Code's settings files that hold permission rules, in its order (highest priority
//! first): your organization's managed settings (read only), each open project's own local file
//! and shared file, and each profile's user settings.

use std::path::{Path, PathBuf};

use super::Rules;

/// Which of Claude Code's settings files.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Scope {
    /// `/Library/Application Support/ClaudeCode/managed-settings.json`: from your organization;
    /// Sawit doesn't change it.
    Managed,
    /// `<project>/.claude/settings.local.json`: yours, for one project, not committed.
    Local(PathBuf),
    /// `<project>/.claude/settings.json`: shared with the team through the repository.
    Project(PathBuf),
    /// `<profile>/settings.json`: yours, for every project; `None` is the default profile
    /// (`~/.claude`), else the profile's name.
    User(Option<String>),
}

impl Scope {
    /// "User", "User · work", "Project", "Project, only you", "Managed".
    pub fn label(&self) -> String {
        match self {
            Scope::Managed => "Managed".into(),
            Scope::Local(_) => "Project, only you".into(),
            Scope::Project(_) => "Project".into(),
            Scope::User(None) => "User".into(),
            Scope::User(Some(p)) => format!("User · {p}"),
        }
    }

    /// The project it belongs to.
    pub fn project(&self) -> Option<&Path> {
        match self {
            Scope::Local(p) | Scope::Project(p) => Some(p),
            _ => None,
        }
    }

    /// Its short name for tools: `user`, `user:work`, `project`, `local`, `managed`.
    pub fn id(&self) -> String {
        match self {
            Scope::Managed => "managed".into(),
            Scope::Local(_) => "local".into(),
            Scope::Project(_) => "project".into(),
            Scope::User(None) => "user".into(),
            Scope::User(Some(p)) => format!("user:{p}"),
        }
    }
}

/// One settings file and its rules.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RulesFile {
    pub scope: Scope,
    pub path: PathBuf,
    pub exists: bool,
    /// Sawit may change it: not the managed file, and writable.
    pub editable: bool,
    pub rules: Rules,
    /// Why its rules couldn't be read, when they couldn't (not JSON, a list that isn't).
    pub problem: Option<String>,
}

/// The rules in a settings file's `text`: `permissions.allow`, `ask` and `deny`. Missing ones
/// are empty; a list that isn't one, or a file that isn't JSON, is an error.
pub fn read_rules(text: &str) -> Result<Rules, String> {
    if text.trim().is_empty() {
        return Ok(Rules::default());
    }
    let root = super::json_edit::parse(text).map_err(|e| format!("Not JSON: {e}"))?;
    if !matches!(root.kind, super::json_edit::Kind::Object(_)) {
        return Err("The file isn't a JSON object.".into());
    }
    let mut rules = Rules::default();
    let Some(perms) = root.member("permissions") else {
        return Ok(rules);
    };
    if !matches!(perms.kind, super::json_edit::Kind::Object(_)) {
        return Err("permissions isn't an object.".into());
    }
    for bucket in super::Bucket::ALL {
        let Some(list) = perms.member(bucket.key()) else {
            continue;
        };
        let super::json_edit::Kind::Array(items) = &list.kind else {
            return Err(format!("permissions.{} isn't a list.", bucket.key()));
        };
        let out = match bucket {
            super::Bucket::Allow => &mut rules.allow,
            super::Bucket::Ask => &mut rules.ask,
            super::Bucket::Deny => &mut rules.deny,
        };
        for item in items {
            if let super::json_edit::Kind::Str(s) = &item.kind {
                out.push(s.clone());
            }
        }
    }
    Ok(rules)
}

/// Claude Code's settings files for `profiles` (each profile's name, `None` for the default,
/// and its folder) and the open `projects`, in Claude Code's order: managed (when `managed`
/// exists), each project's local and shared file, then each profile's user file. A project's
/// files and a profile's are listed even when they aren't there yet: adding a rule creates them.
pub fn claude_files(
    profiles: &[(Option<String>, PathBuf)],
    projects: &[PathBuf],
    managed: &Path,
) -> Vec<RulesFile> {
    let mut out = Vec::new();
    let mut push = |scope: Scope, path: PathBuf| {
        let text = std::fs::read_to_string(&path).ok();
        let exists = text.is_some();
        let (rules, problem) = match read_rules(text.as_deref().unwrap_or("")) {
            Ok(rules) => (rules, None),
            Err(e) => (Rules::default(), Some(e)),
        };
        let editable = scope != Scope::Managed
            && problem.is_none()
            && (!exists || crate::shell_config::writable(&path));
        out.push(RulesFile {
            scope,
            path,
            exists,
            editable,
            rules,
            problem,
        });
    };
    if managed.is_file() {
        push(Scope::Managed, managed.to_path_buf());
    }
    for project in projects {
        push(
            Scope::Local(project.clone()),
            project.join(".claude/settings.local.json"),
        );
        push(
            Scope::Project(project.clone()),
            project.join(".claude/settings.json"),
        );
    }
    for (name, dir) in profiles {
        push(Scope::User(name.clone()), dir.join("settings.json"));
    }
    out
}

/// Your organization's managed settings, which Sawit shows and never changes.
pub const MANAGED: &str = "/Library/Application Support/ClaudeCode/managed-settings.json";

/// Claude Code's profiles on this Mac: the default (`~/.claude`) first, then each other one
/// [`crate::mcp_clients::targets`] finds (its `CLAUDE_CONFIG_DIR` folder), with its name.
pub fn claude_profiles() -> Vec<(Option<String>, PathBuf)> {
    let home = PathBuf::from(std::env::var("HOME").unwrap_or_default());
    let mut out: Vec<(Option<String>, PathBuf)> = vec![(None, home.join(".claude"))];
    for t in crate::mcp_clients::targets() {
        if t.client == crate::mcp_clients::Client::ClaudeCode
            && let Some(dir) = t.profile_dir
            && !out.iter().any(|(_, d)| *d == dir)
        {
            out.push((Some(t.profile), dir));
        }
    }
    out
}

/// The file of `files` a tool names: `user`, `user:<profile>`, `project` or `local` (of
/// `project`, the first project when it's `None`), or `managed`.
pub fn find_file<'a>(
    files: &'a [RulesFile],
    id: &str,
    project: Option<&Path>,
) -> Option<&'a RulesFile> {
    let id = id.trim().to_lowercase();
    files.iter().find(|f| {
        f.scope.id().to_lowercase() == id
            && (project.is_none() || f.scope.project().is_none() || f.scope.project() == project)
    })
}
