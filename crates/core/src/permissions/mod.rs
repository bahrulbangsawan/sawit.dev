//! What agents may do on this Mac, and changing it safely: Claude Code's permission rules
//! (`permissions.allow`, `ask` and `deny` in its settings files, for each profile, each open
//! project and the project's own local file), the other agents' permission settings (read
//! only), which of Sawit's MCP tools ask for confirmation and what Claude Code's rules say of
//! them, and the macOS permissions Sawit needs.
//!
//! An edit is planned first ([`plan`]): the file's new text, made by splicing the rule in or
//! out where it belongs, so every other byte (keys, their order, formatting, comments) stays;
//! and a diff to show. [`save`] then backs the file up beside it (`.sawit-backup`) and writes the
//! new text, unless the file changed since it was read. Rules that look like secrets are refused.

mod claude;
mod diff;
mod json_edit;
mod others;
mod sawit_tools;
#[cfg(test)]
mod tests;

use std::path::{Path, PathBuf};

pub use claude::{MANAGED, RulesFile, Scope, claude_files, claude_profiles, find_file, read_rules};
pub use diff::unified;
pub use others::{OtherAgent, other_agents};
pub use sawit_tools::{CONFIRM_TOOLS, ToolState, sawit_tool_states};

use crate::{CoreError, Result};

/// Where a rule sits: Claude Code allows the tool use without asking, asks first, or refuses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Bucket {
    Allow,
    Ask,
    Deny,
}

impl Bucket {
    pub const ALL: [Bucket; 3] = [Bucket::Allow, Bucket::Ask, Bucket::Deny];

    /// Its key in `permissions`.
    pub fn key(self) -> &'static str {
        match self {
            Bucket::Allow => "allow",
            Bucket::Ask => "ask",
            Bucket::Deny => "deny",
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            Bucket::Allow => "Allow",
            Bucket::Ask => "Ask",
            Bucket::Deny => "Deny",
        }
    }

    pub fn parse(name: &str) -> Option<Bucket> {
        Bucket::ALL
            .into_iter()
            .find(|b| b.key().eq_ignore_ascii_case(name.trim()))
    }
}

/// A file's rules, bucket by bucket, in the file's order.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Rules {
    pub allow: Vec<String>,
    pub ask: Vec<String>,
    pub deny: Vec<String>,
}

impl Rules {
    pub fn get(&self, bucket: Bucket) -> &[String] {
        match bucket {
            Bucket::Allow => &self.allow,
            Bucket::Ask => &self.ask,
            Bucket::Deny => &self.deny,
        }
    }

    /// The bucket `rule` is in, when it's in one.
    pub fn bucket_of(&self, rule: &str) -> Option<Bucket> {
        Bucket::ALL
            .into_iter()
            .find(|b| self.get(*b).iter().any(|r| r == rule))
    }

    pub fn is_empty(&self) -> bool {
        self.allow.is_empty() && self.ask.is_empty() && self.deny.is_empty()
    }
}

/// A change to one file's rules.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Change {
    Add {
        rule: String,
        to: Bucket,
    },
    Remove {
        rule: String,
    },
    /// Takes it out of the bucket it is in and adds it to `to`.
    Move {
        rule: String,
        to: Bucket,
    },
}

impl Change {
    pub fn rule(&self) -> &str {
        match self {
            Change::Add { rule, .. } | Change::Remove { rule } | Change::Move { rule, .. } => rule,
        }
    }

    /// "Add Bash(npm test) to Allow".
    pub fn describe(&self) -> String {
        match self {
            Change::Add { rule, to } => format!("Add {rule} to {}", to.title()),
            Change::Remove { rule } => format!("Remove {rule}"),
            Change::Move { rule, to } => format!("Move {rule} to {}", to.title()),
        }
    }
}

/// A planned edit: the text it was made from, the text it writes, and the diff between them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    pub path: PathBuf,
    pub before: String,
    pub after: String,
    pub diff: String,
}

/// Why `rule` can't be written, if it can't: empty, more than one line, not a permission rule
/// (`Tool` or `Tool(specifier)`), or holding what looks like a secret (an API key or token),
/// which never goes into a settings file.
pub fn check_rule(rule: &str) -> std::result::Result<(), String> {
    let rule = rule.trim();
    if rule.is_empty() {
        return Err("The rule is empty.".into());
    }
    if rule.contains(['\n', '\r']) || rule.len() > 500 {
        return Err("A rule is one line, at most 500 characters.".into());
    }
    let name_end = rule.find('(').unwrap_or(rule.len());
    let name = &rule[..name_end];
    let name_ok = name.starts_with(|c: char| c.is_ascii_alphabetic())
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '*');
    let spec_ok = name_end == rule.len() || rule.ends_with(')');
    if !name_ok || !spec_ok {
        return Err(format!(
            "\"{rule}\" isn't a permission rule: a tool's name (Bash, Read, mcp__sawit__query), \
             optionally with what it applies to in brackets (Bash(npm run test:*))."
        ));
    }
    if looks_secret(rule) {
        return Err(
            "The rule looks like it holds a secret (an API key or a token). Sawit never writes secrets into settings files."
                .into(),
        );
    }
    Ok(())
}

/// Whether `text` holds something like a key or token: a known prefix, a private key, or a long
/// run of letters and digits mixed.
pub fn looks_secret(text: &str) -> bool {
    const PREFIXES: [&str; 10] = [
        "sk-",
        "sk_live_",
        "ghp_",
        "gho_",
        "github_pat_",
        "glpat-",
        "xoxb-",
        "xoxp-",
        "AKIA",
        "-----BEGIN",
    ];
    if PREFIXES.iter().any(|p| text.contains(p)) {
        return true;
    }
    text.split(|c: char| !(c.is_ascii_alphanumeric() || "+/=_-".contains(c)))
        .any(|run| {
            run.len() >= 32
                && run.chars().any(|c| c.is_ascii_digit())
                && run.chars().any(|c| c.is_ascii_alphabetic())
        })
}

/// The text `change` makes of a settings file's `text` (empty for a file not there yet): the
/// rule spliced into or out of `permissions`, everything else as it was. Fails, saying why,
/// when the rule is refused, the file isn't JSON or its `permissions` isn't shaped as Claude
/// Code reads it, the rule is already where it would go, or a rule to take out isn't there.
pub fn apply(text: &str, change: &Change) -> std::result::Result<String, String> {
    let rule = change.rule().trim().to_string();
    if !matches!(change, Change::Remove { .. }) {
        check_rule(&rule)?;
    }
    let base = if text.trim().is_empty() { "{}\n" } else { text };
    let rules = read_rules(base)?;
    match change {
        Change::Add { to, .. } => {
            if let Some(b) = rules.bucket_of(&rule) {
                return Err(format!("{rule} is already in {}.", b.title()));
            }
            add(base, &rule, *to)
        }
        Change::Remove { .. } => remove(base, &rule),
        Change::Move { to, .. } => match rules.bucket_of(&rule) {
            None => Err(format!("{rule} isn't in this file.")),
            Some(b) if b == *to => Err(format!("{rule} is already in {}.", to.title())),
            Some(_) => add(&remove(base, &rule)?, &rule, *to),
        },
    }
}

fn add(text: &str, rule: &str, to: Bucket) -> std::result::Result<String, String> {
    let root = json_edit::parse(text)?;
    let value = serde_json::Value::String(rule.to_string());
    let Some(perms) = root.member("permissions") else {
        let block = serde_json::json!({ to.key(): [rule] });
        return Ok(json_edit::push_member(text, &root, "permissions", &block));
    };
    match perms.member(to.key()) {
        Some(
            list @ json_edit::Node {
                kind: json_edit::Kind::Array(_),
                ..
            },
        ) => Ok(json_edit::push_item(text, list, &value)),
        Some(_) => Err(format!("permissions.{} isn't a list.", to.key())),
        None => Ok(json_edit::push_member(
            text,
            perms,
            to.key(),
            &serde_json::json!([rule]),
        )),
    }
}

fn remove(text: &str, rule: &str) -> std::result::Result<String, String> {
    let root = json_edit::parse(text)?;
    let perms = root.member("permissions");
    for bucket in Bucket::ALL {
        let Some(list) = perms.and_then(|p| p.member(bucket.key())) else {
            continue;
        };
        if let json_edit::Kind::Array(items) = &list.kind
            && let Some(ix) = items
                .iter()
                .position(|n| matches!(&n.kind, json_edit::Kind::Str(s) if s == rule))
        {
            return Ok(json_edit::remove_item(text, list, ix));
        }
    }
    Err(format!("{rule} isn't in this file."))
}

/// Plans `change` to the settings file at `path` (not there yet is fine): its text now, its new
/// text, and the diff. The new text is checked to read back as JSON with the change made.
pub fn plan(path: &Path, change: &Change) -> std::result::Result<Plan, String> {
    let before = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(format!("Couldn't read {}: {e}", path.display())),
    };
    let after = apply(&before, change)?;
    // The result must be JSON, with the rule where the change put it.
    serde_json::from_str::<serde_json::Value>(&after)
        .map_err(|e| format!("The change would break the file ({e}); nothing was written."))?;
    let rules = read_rules(&after)?;
    let rule = change.rule().trim();
    let expected = match change {
        Change::Add { to, .. } | Change::Move { to, .. } => Some(*to),
        Change::Remove { .. } => None,
    };
    if rules.bucket_of(rule) != expected {
        return Err("The change didn't come out as planned; nothing was written.".into());
    }
    Ok(Plan {
        path: path.to_path_buf(),
        diff: unified(&before, &after, &path.display().to_string()),
        before,
        after,
    })
}

/// Writes `plan`: backs the file up first (`<file>.sawit-backup`, when there is one), then
/// writes its new text, creating the folder for a new project file. Refused when the file
/// changed since the plan read it.
pub fn save(plan: &Plan) -> Result<()> {
    let now = match std::fs::read_to_string(&plan.path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(e.into()),
    };
    if now != plan.before {
        return Err(CoreError::Config(format!(
            "{} changed since it was read: look again before saving.",
            plan.path.display()
        )));
    }
    if plan.path.exists() {
        std::fs::copy(&plan.path, backup_path(&plan.path))?;
    } else if let Some(dir) = plan.path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    crate::files::write_text_atomic(&plan.path, &plan.after)
}

/// `<file>.sawit-backup`: the file as it was before Sawit's last change to it.
pub fn backup_path(path: &Path) -> PathBuf {
    crate::mcp_clients::backup_path(path)
}
