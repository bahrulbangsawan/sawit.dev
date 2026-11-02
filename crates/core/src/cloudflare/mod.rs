//! Cloudflare Workers projects, as Sawit finds them on disk: the folders of a project that hold a
//! Worker (a Wrangler config, a `cloudflare.config.ts`, or the Cloudflare Vite plugin in
//! `package.json`), the bindings each one declares, the dev tool that serves it and the version
//! of that tool installed in `node_modules`, so the app can say whether the dev server will have
//! Local Explorer. Finding a running Local Explorer and calling its API is [`explorer`]'s.
//!
//! Nothing here runs Wrangler, Vite or `cf`: the files are read as they are, and a
//! `cloudflare.config.ts` only as far as it is plain literals.

pub mod explorer;

mod config;
mod scripts;
mod support;
mod tool;

use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::scripts::{expand_workspaces, read_json, workspace_patterns};
use config::read_config;
use scripts::persist_to;

pub use scripts::{DevScript, dev_scripts, starts_dev_server};
pub use support::{Support, declares_crons, requirement_note, support, upgrade_command};
pub use tool::{DevTool, Version};

/// The files that hold a Worker's config, in the order a folder's one is picked: `cf`'s own first,
/// then the ones Wrangler reads, JSON before TOML as Wrangler prefers them.
pub const CONFIG_FILES: [&str; 4] = [
    "cloudflare.config.ts",
    "wrangler.json",
    "wrangler.jsonc",
    "wrangler.toml",
];

/// The package that runs a Worker inside Vite's dev server.
pub const VITE_PLUGIN: &str = "@cloudflare/vite-plugin";

/// At most this many folders beside the project's own are looked into for a Worker of their own.
const MAX_CHILDREN: usize = 200;

/// A kind of binding a Worker declares.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum BindingKind {
    D1,
    Kv,
    R2,
    DurableObject,
    Workflow,
    SendEmail,
    /// Another kind, by its config key's name: `service`, `queue`, `hyperdrive`, `ai`, ….
    Other(String),
}

impl BindingKind {
    /// How Local Explorer and Cloudflare's docs name it.
    pub fn label(&self) -> &str {
        match self {
            BindingKind::D1 => "D1",
            BindingKind::Kv => "KV",
            BindingKind::R2 => "R2",
            BindingKind::DurableObject => "Durable Objects",
            BindingKind::Workflow => "Workflows",
            BindingKind::SendEmail => "send_email",
            BindingKind::Other(kind) => kind,
        }
    }

    /// Whether Local Explorer shows its data (or, for `send_email`, the mail it sent).
    pub fn in_explorer(&self) -> bool {
        !matches!(self, BindingKind::Other(_))
    }

    fn from_tag(tag: &str) -> BindingKind {
        match tag {
            "d1" => BindingKind::D1,
            "kv" => BindingKind::Kv,
            "r2" => BindingKind::R2,
            "durable_object" | "durableObject" => BindingKind::DurableObject,
            "workflow" => BindingKind::Workflow,
            "send_email" | "sendEmail" => BindingKind::SendEmail,
            other => BindingKind::Other(other.to_string()),
        }
    }
}

/// One binding of a Worker.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Binding {
    pub kind: BindingKind,
    /// The name the Worker reads it by, in `env` (`DB`).
    pub binding: String,
    /// The resource's own name when the config gives one: a D1 `database_name`, an R2
    /// `bucket_name`, a Durable Object's `class_name`, a Workflow's `name`.
    pub name: Option<String>,
    /// The resource's id when the config gives one: a D1 `database_id`, a KV namespace `id`.
    pub id: Option<String>,
    /// A D1 binding's `preview_database_id`: the database `wrangler dev` uses locally in its
    /// place, which Miniflare keeps the local data under ([`crate::db::d1::miniflare_id`]).
    pub preview_id: Option<String>,
}

/// One Worker of a project: a folder with its own config.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkersProject {
    /// The folder the Worker is in: the project's, or one of its packages'.
    pub root: PathBuf,
    /// Its config file. A Vite plugin project with no config of its own has its `package.json`
    /// here, which is what marks it.
    pub config: PathBuf,
    /// What serves it in development.
    pub tool: DevTool,
    /// That tool's version in `node_modules`, from the folder up to the project's root.
    pub version: Option<String>,
    /// The Worker's name, when the config gives it as a literal.
    pub name: Option<String>,
    /// The bindings the config declares for local development (its top level, not `env.*`).
    pub bindings: Vec<Binding>,
    /// Where a dev script keeps local data (`--persist-to`), relative paths taken from `root`.
    pub persist_to: Option<PathBuf>,
}

impl WorkersProject {
    /// The kinds of its bindings that Local Explorer shows, once each, in the config's order.
    pub fn explorer_kinds(&self) -> Vec<&BindingKind> {
        let mut kinds: Vec<&BindingKind> = Vec::new();
        for b in self.bindings.iter().filter(|b| b.kind.in_explorer()) {
            if !kinds.contains(&&b.kind) {
                kinds.push(&b.kind);
            }
        }
        kinds
    }

    /// Its config file's name (`wrangler.jsonc`).
    pub fn config_name(&self) -> String {
        self.config
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default()
    }
}

/// Every Worker of the project at `root`: the root's own, each workspace package's (`workspaces`
/// in `package.json`, `pnpm-workspace.yaml`), and those of the folders right inside the root that
/// hold a Worker config, as `cf`'s docs lay out several Workers (`api/`, `web/`).
pub fn detect_project(root: &Path) -> Vec<WorkersProject> {
    let manifest = read_json(&root.join("package.json")).unwrap_or(Value::Null);
    let mut dirs = vec![root.to_path_buf()];
    for dir in expand_workspaces(root, &workspace_patterns(root, &manifest))
        .into_iter()
        .chain(folders_with_config(root))
    {
        if !dirs.contains(&dir) {
            dirs.push(dir);
        }
    }
    dirs.iter()
        .filter_map(|dir| detect_folder(dir, root))
        .collect()
}

/// The folders right inside `root` (not hidden, not `node_modules`) that hold a Worker config.
fn folders_with_config(root: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(root) else {
        return Vec::new();
    };
    let mut found: Vec<PathBuf> = entries
        .flatten()
        .take(MAX_CHILDREN)
        .filter(|e| {
            let name = e.file_name();
            let name = name.to_string_lossy();
            !name.starts_with('.') && name != "node_modules"
        })
        .map(|e| e.path())
        .filter(|p| p.is_dir() && CONFIG_FILES.iter().any(|f| p.join(f).is_file()))
        .collect();
    found.sort();
    found
}

/// The Worker in `dir`, a folder of the project at `root`, if it holds one.
fn detect_folder(dir: &Path, root: &Path) -> Option<WorkersProject> {
    let manifest = read_json(&dir.join("package.json")).unwrap_or(Value::Null);
    let declares = |package: &str| {
        [
            "dependencies",
            "devDependencies",
            "optionalDependencies",
            "peerDependencies",
        ]
        .iter()
        .any(|key| manifest.get(key).and_then(|d| d.get(package)).is_some())
    };
    let config = CONFIG_FILES
        .iter()
        .map(|f| dir.join(f))
        .find(|p| p.is_file());
    let vite = declares(VITE_PLUGIN);
    if config.is_none() && !vite {
        return None;
    }
    let config = config.unwrap_or_else(|| dir.join("package.json"));
    let cf_config = config.ends_with("cloudflare.config.ts");
    let tool = if vite {
        DevTool::VitePlugin
    } else if cf_config && !declares("wrangler") {
        DevTool::CfCli
    } else {
        DevTool::Wrangler
    };
    let read = read_config(&config);
    Some(WorkersProject {
        root: dir.to_path_buf(),
        tool,
        version: installed_version(dir, root, tool.package()),
        name: read.name,
        bindings: read.bindings,
        persist_to: persist_to(dir, &manifest, tool),
        config,
    })
}

/// The version of `package` that Node finds from `dir`: in `node_modules` of `dir` or a folder
/// above it, up to the project's `root` (a workspace hoists its packages there). Folders above the
/// project aren't looked into, though Node would: a stray `~/node_modules` isn't the project's.
pub fn installed_version(dir: &Path, root: &Path, package: &str) -> Option<String> {
    let mut at = Some(dir);
    while let Some(d) = at {
        let manifest = d.join("node_modules").join(package).join("package.json");
        if let Some(version) =
            read_json(&manifest).and_then(|m| m.get("version")?.as_str().map(str::to_string))
        {
            return Some(version);
        }
        if d == root || !d.starts_with(root) {
            break;
        }
        at = d.parent();
    }
    None
}

#[cfg(test)]
mod tests;
