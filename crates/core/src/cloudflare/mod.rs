//! Cloudflare Workers projects, as Sawit finds them on disk: the folders of a project that hold a
//! Worker (a Wrangler config, a `cloudflare.config.ts`, or the Cloudflare Vite plugin in
//! `package.json`), the bindings each one declares, the dev tool that serves it and the version
//! of that tool installed in `node_modules`, so the app can say whether the dev server will have
//! Local Explorer. Finding a running Local Explorer and calling its API is [`explorer`]'s.
//!
//! Nothing here runs Wrangler, Vite or `cf`: the files are read as they are, and a
//! `cloudflare.config.ts` only as far as it is plain literals.

pub mod explorer;

use std::cmp::Ordering;
use std::fmt;
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::package_manager::PackageManager;
use crate::scripts::{expand_workspaces, read_json, workspace_patterns};

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

/// What serves a Worker in development, and so Local Explorer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DevTool {
    /// `wrangler dev`.
    Wrangler,
    /// `vite` with `@cloudflare/vite-plugin`.
    VitePlugin,
    /// `cf dev` for a `cloudflare.config.ts` project that declares neither of the others, which
    /// then starts one of them itself.
    CfCli,
}

impl DevTool {
    /// Its npm package, whose `package.json` in `node_modules` says the installed version.
    pub fn package(self) -> &'static str {
        match self {
            DevTool::Wrangler => "wrangler",
            DevTool::VitePlugin => VITE_PLUGIN,
            DevTool::CfCli => "cf",
        }
    }

    /// Its name in a sentence.
    pub fn name(self) -> &'static str {
        match self {
            DevTool::Wrangler => "Wrangler",
            DevTool::VitePlugin => "the Cloudflare Vite plugin",
            DevTool::CfCli => "cf",
        }
    }

    /// The first version whose dev server serves Local Explorer. `cf` has none of its own: the
    /// tool it starts needs this.
    pub fn explorer_since(self) -> Option<Version> {
        match self {
            DevTool::Wrangler => Some(Version::new(4, 118, 0)),
            DevTool::VitePlugin => Some(Version::new(1, 50, 0)),
            DevTool::CfCli => None,
        }
    }

    /// The first version whose Local Explorer can invoke a Worker's Cron Triggers.
    pub fn cron_since(self) -> Option<Version> {
        match self {
            DevTool::Wrangler => Some(Version::new(4, 139, 0)),
            DevTool::VitePlugin => Some(Version::new(1, 60, 0)),
            DevTool::CfCli => None,
        }
    }
}

/// An npm package's version, as far as Local Explorer's requirements need one: `4.118.0`, with a
/// pre-release (`4.118.0-beta.1`) before the release it leads to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Version {
    pub major: u64,
    pub minor: u64,
    pub patch: u64,
    pub pre: bool,
}

impl Version {
    pub const fn new(major: u64, minor: u64, patch: u64) -> Version {
        Version {
            major,
            minor,
            patch,
            pre: false,
        }
    }

    /// `4.118.0`, `v4.118`, `4.118.0-beta.1` or `4.118.0+build`; a missing minor or patch is 0.
    pub fn parse(text: &str) -> Option<Version> {
        let text = text.trim().trim_start_matches(['v', 'V', '=']);
        // Build metadata (`+build.7`) doesn't order versions.
        let core = text.split_once('+').map_or(text, |(core, _)| core);
        let (core, pre) = match core.split_once('-') {
            Some((core, pre)) => (core, !pre.is_empty()),
            None => (core, false),
        };
        let mut parts = core.split('.');
        let major = parts.next()?.parse().ok()?;
        let minor = parts.next().map_or(Some(0), |p| p.parse().ok())?;
        let patch = parts.next().map_or(Some(0), |p| p.parse().ok())?;
        if parts.next().is_some() {
            return None;
        }
        Some(Version {
            major,
            minor,
            patch,
            pre,
        })
    }
}

impl Ord for Version {
    fn cmp(&self, other: &Self) -> Ordering {
        (self.major, self.minor, self.patch)
            .cmp(&(other.major, other.minor, other.patch))
            // A pre-release comes before its release.
            .then((!self.pre).cmp(&!other.pre))
    }
}

impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

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

/// Whether the Worker's installed dev tool serves Local Explorer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Support {
    /// It does; `cron` says whether it can also invoke Cron Triggers.
    Ready { cron: bool },
    /// The installed version is older than the first one with Local Explorer.
    TooOld { installed: Version, needed: Version },
    /// Nothing says: the tool isn't installed yet, its version doesn't parse, or it is `cf`,
    /// which starts another tool.
    Unknown,
}

/// Whether `project`'s installed dev tool serves Local Explorer.
pub fn support(project: &WorkersProject) -> Support {
    let (Some(needed), Some(installed)) = (
        project.tool.explorer_since(),
        project.version.as_deref().and_then(Version::parse),
    ) else {
        return Support::Unknown;
    };
    if installed < needed {
        return Support::TooOld { installed, needed };
    }
    Support::Ready {
        cron: project.tool.cron_since().is_some_and(|v| installed >= v),
    }
}

/// What to tell someone about `project`'s dev tool and Local Explorer, when there is something:
/// that its version has none and the command that upgrades it, that Cron Triggers need a newer
/// one, or that it isn't installed yet. `root` is the project's.
pub fn requirement_note(project: &WorkersProject, root: &Path) -> Option<String> {
    let tool = project.tool;
    match support(project) {
        Support::TooOld { installed, needed } => Some(format!(
            "{} {installed} has no Local Explorer: it needs {needed} or later. Upgrade with `{}`.",
            capitalized(tool.name()),
            upgrade_command(project, root)
        )),
        Support::Ready { cron: false } if declares_crons(project) => {
            tool.cron_since().map(|since| {
                format!(
                    "Invoking Cron Triggers from Local Explorer needs {} {since} or later; upgrade with `{}`.",
                    tool.name(),
                    upgrade_command(project, root)
                )
            })
        }
        Support::Unknown if project.version.is_none() && tool != DevTool::CfCli => {
            tool.explorer_since().map(|since| {
                format!(
                    "{} isn't in node_modules yet (install the project's dependencies); Local Explorer needs {since} or later.",
                    capitalized(tool.name())
                )
            })
        }
        _ => None,
    }
}

fn capitalized(text: &str) -> String {
    let mut chars = text.chars();
    chars
        .next()
        .map(|c| c.to_uppercase().chain(chars).collect())
        .unwrap_or_default()
}

/// Whether `project`'s config declares Cron Triggers (`triggers.crons`, or
/// `triggers.scheduled(…)` in a `cloudflare.config.ts`).
pub fn declares_crons(project: &WorkersProject) -> bool {
    read_config(&project.config).crons
}

/// The command that brings `project`'s dev tool up to date, with the package manager the project
/// uses (its `packageManager` field, else its lockfile, else npm): `pnpm add -D wrangler@latest`.
/// `root` is the project's, where a workspace keeps its lockfile. Sawit shows it and runs it only
/// when asked.
pub fn upgrade_command(project: &WorkersProject, root: &Path) -> String {
    let manifest = read_json(&project.root.join("package.json")).unwrap_or(Value::Null);
    let pm = PackageManager::parse(crate::scripts::package_manager(
        &project.root,
        root,
        &manifest,
    ))
    .unwrap_or(PackageManager::Npm);
    let package = format!("{}@latest", project.tool.package());
    let mut command = pm.dev_install(&package);
    // pnpm adds to a workspace's root only when told it means to.
    if pm == PackageManager::Pnpm && project.root.join("pnpm-workspace.yaml").is_file() {
        command.push_str(" -w");
    }
    command
}

/// A `package.json` script that starts the Worker's dev server, and so Local Explorer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DevScript {
    pub name: String,
    /// What it runs: `wrangler dev --port 8787`.
    pub body: String,
}

/// The scripts of `project`'s `package.json` that start its dev server: `dev` first, then
/// `start`, then the others in the file's order.
pub fn dev_scripts(project: &WorkersProject) -> Vec<DevScript> {
    let manifest = read_json(&project.root.join("package.json")).unwrap_or(Value::Null);
    let mut found: Vec<DevScript> = scripts_of(&manifest)
        .into_iter()
        .filter(|(_, body)| starts_dev_server(body, project.tool))
        .map(|(name, body)| DevScript { name, body })
        .collect();
    found.sort_by_key(|s| match s.name.as_str() {
        "dev" => 0,
        "start" => 1,
        _ => 2,
    });
    found
}

fn scripts_of(manifest: &Value) -> Vec<(String, String)> {
    manifest
        .get("scripts")
        .and_then(Value::as_object)
        .map(|o| {
            o.iter()
                .filter_map(|(k, v)| Some((k.clone(), v.as_str()?.to_string())))
                .collect()
        })
        .unwrap_or_default()
}

/// Whether the command line `body` starts a Workers dev server: `wrangler dev`, `cf dev`, or
/// `vite` serving (not `vite build` or `vite preview`); with the Vite plugin also a framework's
/// own dev command that runs Vite (`react-router dev`, `astro dev`, `vinext dev`). Each command of
/// a `&&`, `;` or `|` chain counts, quoted ones (`concurrently "wrangler dev"`) too.
pub fn starts_dev_server(body: &str, tool: DevTool) -> bool {
    body.split(['&', '|', ';']).any(|segment| {
        let words: Vec<&str> = segment
            .split_whitespace()
            .map(|w| w.trim_matches(|c| c == '"' || c == '\''))
            .filter(|w| !w.is_empty())
            .map(|w| w.rsplit('/').next().unwrap_or(w))
            .collect();
        words.iter().enumerate().any(|(ix, word)| {
            let next = words.get(ix + 1).copied();
            match *word {
                "wrangler" | "cf" => next == Some("dev"),
                "vite" => next.is_none_or(|n| n == "dev" || n == "serve" || n.starts_with('-')),
                "react-router" | "astro" | "vinext" => {
                    tool == DevTool::VitePlugin && next == Some("dev")
                }
                _ => false,
            }
        })
    })
}

/// The `--persist-to` folder of the first dev script that gives one, else of any script.
fn persist_to(dir: &Path, manifest: &Value, tool: DevTool) -> Option<PathBuf> {
    let mut scripts = scripts_of(manifest);
    scripts.sort_by_key(|(_, body)| !starts_dev_server(body, tool));
    scripts.iter().find_map(|(_, body)| {
        let words: Vec<&str> = body.split_whitespace().collect();
        let value =
            words
                .iter()
                .enumerate()
                .find_map(|(ix, w)| match w.strip_prefix("--persist-to") {
                    Some(rest) if rest.starts_with('=') => Some(rest[1..].to_string()),
                    Some("") => words.get(ix + 1).map(|v| v.to_string()),
                    _ => None,
                })?;
        let value = value.trim_matches(|c| c == '"' || c == '\'');
        (!value.is_empty()).then(|| dir.join(value))
    })
}

/// What a config file says that Sawit shows.
#[derive(Debug, Default)]
struct Config {
    name: Option<String>,
    bindings: Vec<Binding>,
    crons: bool,
}

fn read_config(path: &Path) -> Config {
    let file = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_default();
    match file {
        "wrangler.json" | "wrangler.jsonc" => read_json(path)
            .map(|v| from_wrangler(&v))
            .unwrap_or_default(),
        "wrangler.toml" => std::fs::read_to_string(path)
            .ok()
            .and_then(|text| toml::from_str::<toml::Table>(&text).ok())
            .and_then(|table| serde_json::to_value(table).ok())
            .map(|v| from_wrangler(&v))
            .unwrap_or_default(),
        "cloudflare.config.ts" => std::fs::read_to_string(path)
            .map(|text| typescript::read(&text))
            .unwrap_or_default(),
        _ => Config::default(),
    }
}

/// One binding list of a Wrangler config: where it lives, its kind, and which keys of an entry
/// hold the binding's name, the resource's name and its id.
struct BindingList {
    at: &'static str,
    tag: &'static str,
    binding: &'static str,
    name: Option<&'static str>,
    id: Option<&'static str>,
}

const fn list(
    at: &'static str,
    tag: &'static str,
    binding: &'static str,
    name: Option<&'static str>,
    id: Option<&'static str>,
) -> BindingList {
    BindingList {
        at,
        tag,
        binding,
        name,
        id,
    }
}

/// The binding lists Sawit reads, D1 first.
const WRANGLER_BINDINGS: &[BindingList] = &[
    list(
        "/d1_databases",
        "d1",
        "binding",
        Some("database_name"),
        Some("database_id"),
    ),
    list("/kv_namespaces", "kv", "binding", None, Some("id")),
    list("/r2_buckets", "r2", "binding", Some("bucket_name"), None),
    list(
        "/durable_objects/bindings",
        "durable_object",
        "name",
        Some("class_name"),
        None,
    ),
    list("/workflows", "workflow", "binding", Some("name"), None),
    list(
        "/send_email",
        "send_email",
        "name",
        Some("destination_address"),
        None,
    ),
    list("/services", "service", "binding", Some("service"), None),
    list("/queues/producers", "queue", "binding", Some("queue"), None),
    list("/hyperdrive", "hyperdrive", "binding", None, Some("id")),
    list(
        "/vectorize",
        "vectorize",
        "binding",
        Some("index_name"),
        None,
    ),
    list(
        "/analytics_engine_datasets",
        "analytics_engine",
        "binding",
        Some("dataset"),
        None,
    ),
];

/// The bindings that are one object, not a list.
const WRANGLER_SINGLE: &[(&str, &str)] = &[
    ("/ai", "ai"),
    ("/browser", "browser"),
    ("/images", "images"),
];

/// A Wrangler config (JSON, or TOML read as JSON): its top level, which is what `wrangler dev`
/// uses without `--env`.
fn from_wrangler(config: &Value) -> Config {
    let text = |o: &Value, key: &str| o.get(key).and_then(Value::as_str).map(str::to_string);
    let mut bindings = Vec::new();
    for list in WRANGLER_BINDINGS {
        for item in config
            .pointer(list.at)
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            if let Some(binding) = text(item, list.binding) {
                bindings.push(Binding {
                    kind: BindingKind::from_tag(list.tag),
                    binding,
                    name: list.name.and_then(|k| text(item, k)),
                    id: list.id.and_then(|k| text(item, k)),
                    preview_id: (list.tag == "d1")
                        .then(|| text(item, "preview_database_id"))
                        .flatten(),
                });
            }
        }
    }
    for &(at, tag) in WRANGLER_SINGLE {
        if let Some(binding) = config.pointer(at).and_then(|o| text(o, "binding")) {
            bindings.push(Binding {
                kind: BindingKind::from_tag(tag),
                binding,
                name: None,
                id: None,
                preview_id: None,
            });
        }
    }
    Config {
        name: text(config, "name"),
        bindings,
        crons: config
            .pointer("/triggers/crons")
            .and_then(Value::as_array)
            .is_some_and(|crons| !crons.is_empty()),
    }
}

/// Reading a `cloudflare.config.ts` without running it: its tokens, comments dropped, and the
/// parts written as literals. `defineConfig({ worker: { name: "app", env: { DB: bindings.d1({
/// name: "app-db", id: "…" }) }, triggers: [triggers.scheduled({ schedule: "* * * * *" })] } })`
/// gives the name `app`, a D1 binding `DB` and Cron Triggers; a name worked out at run time
/// (`isStaging ? … : …`) gives none.
mod typescript {
    use super::{Binding, BindingKind, Config};

    #[derive(Debug, Clone, PartialEq)]
    enum Token {
        Ident(String),
        /// A string literal; `None` for a template with `${…}` in it, which isn't known until it
        /// runs.
        Str(Option<String>),
        Punct(char),
    }

    pub(super) fn read(text: &str) -> Config {
        let tokens = tokens(text);
        Config {
            name: worker_object(&tokens)
                .and_then(|open| top_level_string(&tokens, open, &["name"])),
            bindings: bindings(&tokens),
            crons: tokens.windows(3).any(|w| {
                w[0] == Token::Ident("triggers".into())
                    && w[1] == Token::Punct('.')
                    && w[2] == Token::Ident("scheduled".into())
            }),
        }
    }

    fn tokens(text: &str) -> Vec<Token> {
        let chars: Vec<char> = text.chars().collect();
        let mut out = Vec::new();
        let mut ix = 0;
        while ix < chars.len() {
            let c = chars[ix];
            if c.is_whitespace() {
                ix += 1;
            } else if c == '/' && chars.get(ix + 1) == Some(&'/') {
                while ix < chars.len() && chars[ix] != '\n' {
                    ix += 1;
                }
            } else if c == '/' && chars.get(ix + 1) == Some(&'*') {
                ix += 2;
                while ix < chars.len() && !(chars[ix] == '*' && chars.get(ix + 1) == Some(&'/')) {
                    ix += 1;
                }
                ix += 2;
            } else if c == '"' || c == '\'' || c == '`' {
                let mut value = String::new();
                let mut known = true;
                ix += 1;
                while ix < chars.len() && chars[ix] != c {
                    match chars[ix] {
                        '\\' => {
                            if let Some(&n) = chars.get(ix + 1) {
                                value.push(n);
                            }
                            ix += 2;
                            continue;
                        }
                        '$' if c == '`' && chars.get(ix + 1) == Some(&'{') => known = false,
                        other => value.push(other),
                    }
                    ix += 1;
                }
                ix += 1;
                out.push(Token::Str(known.then_some(value)));
            } else if c.is_alphanumeric() || c == '_' || c == '$' {
                let start = ix;
                while ix < chars.len()
                    && (chars[ix].is_alphanumeric() || chars[ix] == '_' || chars[ix] == '$')
                {
                    ix += 1;
                }
                out.push(Token::Ident(chars[start..ix].iter().collect()));
            } else {
                out.push(Token::Punct(c));
                ix += 1;
            }
        }
        out
    }

    fn ident(token: Option<&Token>, name: &str) -> bool {
        matches!(token, Some(Token::Ident(i)) if i == name)
    }

    fn punct(token: Option<&Token>, c: char) -> bool {
        token == Some(&Token::Punct(c))
    }

    /// Where the Worker's object opens: `worker: {` or `defineWorker({`.
    fn worker_object(tokens: &[Token]) -> Option<usize> {
        (0..tokens.len()).find_map(|ix| {
            let (a, b, c) = (tokens.get(ix), tokens.get(ix + 1), tokens.get(ix + 2));
            let opens = (ident(a, "worker") && punct(b, ':') && punct(c, '{'))
                || (ident(a, "defineWorker") && punct(b, '(') && punct(c, '{'));
            opens.then_some(ix + 2)
        })
    }

    /// The literal of the first of `keys` set at the top level of the object opening at `open`.
    fn top_level_string(tokens: &[Token], open: usize, keys: &[&str]) -> Option<String> {
        let mut depth = 0usize;
        let mut found: Vec<(usize, String)> = Vec::new();
        for ix in open..tokens.len() {
            match &tokens[ix] {
                Token::Punct('{' | '(' | '[') => depth += 1,
                Token::Punct('}' | ')' | ']') => {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                }
                Token::Ident(key) | Token::Str(Some(key)) if depth == 1 => {
                    let rank = keys.iter().position(|k| k == key);
                    if let (Some(rank), true, Some(Token::Str(Some(value)))) =
                        (rank, punct(tokens.get(ix + 1), ':'), tokens.get(ix + 2))
                        && (ix == open + 1 || punct(tokens.get(ix - 1), ','))
                    {
                        found.push((rank, value.clone()));
                    }
                }
                _ => {}
            }
        }
        found.sort_by_key(|(rank, _)| *rank);
        found.into_iter().next().map(|(_, v)| v)
    }

    /// Every `KEY: bindings.kind(…)`, skipping plain values and secrets (`text`, `json`,
    /// `secret`), which have no data to look at.
    fn bindings(tokens: &[Token]) -> Vec<Binding> {
        let mut out = Vec::new();
        for ix in 0..tokens.len() {
            if !(ident(tokens.get(ix), "bindings") && punct(tokens.get(ix + 1), '.')) {
                continue;
            }
            let Some(Token::Ident(method)) = tokens.get(ix + 2) else {
                continue;
            };
            // `bindings.queue<Job>(…)`: the type arguments come before the call.
            let mut call = ix + 3;
            if punct(tokens.get(call), '<') {
                let mut depth = 0usize;
                while let Some(token) = tokens.get(call) {
                    match token {
                        Token::Punct('<') => depth += 1,
                        Token::Punct('>') => depth -= 1,
                        _ => {}
                    }
                    call += 1;
                    if depth == 0 {
                        break;
                    }
                }
            }
            if !punct(tokens.get(call), '(')
                || matches!(method.as_str(), "text" | "json" | "secret")
            {
                continue;
            }
            let key = match (ix.checked_sub(2).map(|k| &tokens[k]), ix.checked_sub(1)) {
                (Some(Token::Ident(k) | Token::Str(Some(k))), Some(colon))
                    if punct(tokens.get(colon), ':') =>
                {
                    k.clone()
                }
                _ => continue,
            };
            let args = punct(tokens.get(call + 1), '{').then_some(call + 1);
            let read = |keys: &[&str]| args.and_then(|open| top_level_string(tokens, open, keys));
            out.push(Binding {
                kind: BindingKind::from_tag(method),
                binding: key,
                name: read(&[
                    "name",
                    "databaseName",
                    "bucket",
                    "bucketName",
                    "exportName",
                    "className",
                    "destinationAddress",
                ]),
                id: read(&["id", "databaseId", "namespaceId"]),
                // cloudflare.config.ts documents no preview id.
                preview_id: None,
            });
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(path: &Path, text: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    /// A D1 binding's `preview_database_id` is read beside its `database_id`: `wrangler dev` uses
    /// the preview database locally, and Miniflare names its file by it.
    #[test]
    fn a_d1_bindings_preview_database_id_is_read() {
        let dir = tempfile::tempdir().unwrap();
        write(
            &dir.path().join("wrangler.toml"),
            "name = \"api\"\n[[d1_databases]]\nbinding = \"DB\"\ndatabase_name = \"shop\"\n\
             database_id = \"prod-1\"\npreview_database_id = \"dev-1\"\n",
        );
        let b = &detect_project(dir.path())[0].bindings[0];
        assert_eq!(b.id.as_deref(), Some("prod-1"));
        assert_eq!(b.preview_id.as_deref(), Some("dev-1"));
    }

    fn d1(binding: &str, name: &str, id: Option<&str>) -> Binding {
        Binding {
            kind: BindingKind::D1,
            binding: binding.into(),
            name: Some(name.into()),
            id: id.map(Into::into),
            preview_id: None,
        }
    }

    /// A `wrangler.toml` with every kind Local Explorer shows, the D1 database's name and id,
    /// the Worker's name and its crons; an `[env.staging]` binding isn't `wrangler dev`'s.
    #[test]
    fn a_wrangler_toml_gives_its_bindings() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write(
            &root.join("wrangler.toml"),
            r#"
name = "shop-api"
main = "src/index.ts"

[[kv_namespaces]]
binding = "CACHE"
id = "a1b2"

[[r2_buckets]]
binding = "FILES"
bucket_name = "shop-files"

[[d1_databases]]
binding = "DB"
database_name = "shop-db"
database_id = "1111-2222"

[durable_objects]
bindings = [{ name = "COUNTER", class_name = "Counter" }]

[[workflows]]
binding = "ORDERS"
name = "order-flow"
class_name = "OrderFlow"

[[send_email]]
name = "MAIL"

[triggers]
crons = ["0 * * * *"]

[[env.staging.d1_databases]]
binding = "STAGING_DB"
database_name = "staging"
database_id = "x"
"#,
        );
        let found = detect_project(root);
        assert_eq!(found.len(), 1);
        let w = &found[0];
        assert_eq!(w.name.as_deref(), Some("shop-api"));
        assert_eq!(w.tool, DevTool::Wrangler);
        assert_eq!(w.config, root.join("wrangler.toml"));
        let kinds: Vec<&str> = w.explorer_kinds().iter().map(|k| k.label()).collect();
        assert_eq!(
            kinds,
            [
                "D1",
                "KV",
                "R2",
                "Durable Objects",
                "Workflows",
                "send_email"
            ]
        );
        assert_eq!(w.bindings[0], d1("DB", "shop-db", Some("1111-2222")));
        let counter = w.bindings.iter().find(|b| b.binding == "COUNTER").unwrap();
        assert_eq!(counter.name.as_deref(), Some("Counter"));
        assert!(!w.bindings.iter().any(|b| b.binding == "STAGING_DB"));
        assert!(declares_crons(w));
    }

    /// `wrangler.jsonc` with comments, a URL whose `//` isn't one, and trailing commas.
    #[test]
    fn a_wrangler_jsonc_with_comments_gives_its_bindings() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write(
            &root.join("wrangler.jsonc"),
            r#"// The shop's API.
{
  /* Its name on Cloudflare. */
  "name": "shop-api", // trailing comment
  "vars": { "ORIGIN": "https://example.com" },
  "d1_databases": [
    { "binding": "DB", "database_name": "shop-db", "database_id": "1111-2222", },
  ],
  "kv_namespaces": [{ "binding": "CACHE", "id": "a1b2" }],
}
"#,
        );
        let w = &detect_project(root)[0];
        assert_eq!(w.name.as_deref(), Some("shop-api"));
        assert_eq!(w.bindings[0], d1("DB", "shop-db", Some("1111-2222")));
        assert_eq!(w.bindings[1].kind, BindingKind::Kv);
        assert!(!declares_crons(w));
        // Plain `wrangler.json` too.
        let other = tempfile::tempdir().unwrap();
        write(
            &other.path().join("wrangler.json"),
            r#"{"name":"x","r2_buckets":[{"binding":"B","bucket_name":"b"}]}"#,
        );
        let w = &detect_project(other.path())[0];
        assert_eq!(w.explorer_kinds(), [&BindingKind::R2]);
    }

    /// `cloudflare.config.ts` read as far as it is literals: the Worker's name, its bindings by
    /// their keys (values and secrets left out), the D1 database's name and id, and its crons.
    /// Commented-out bindings don't count, and with no Wrangler declared `cf dev` serves it.
    #[test]
    fn a_cloudflare_config_ts_is_read_without_running_it() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write(
            &root.join("cloudflare.config.ts"),
            r#"import { bindings, defineConfig, triggers } from "cf/config";

// The API Worker; see https://developers.cloudflare.com/cf/.
export default defineConfig({
  worker: {
    name: "example-worker",
    compatibilityDate: "2026-10-01",
    env: {
      API_ORIGIN: bindings.text("https://api.example.com"),
      API_KEY: bindings.secret(),
      DATABASE: bindings.d1({ name: "application-db", id: "9f1c-42" }),
      CACHE: bindings.kv({ id: "kv-1" }),
      "UPLOADS": bindings.r2({ bucket: "uploads" }),
      MAIL: bindings.sendEmail(),
      ORDERS: bindings.workflow({ name: "order-workflow", worker: "example-worker", exportName: "OrderWorkflow" }),
      // OLD: bindings.d1({ name: "old" }),
      /* GONE: bindings.kv({ id: "x" }), */
      JOBS: bindings.queue<Job>({ name: "application-jobs" }),
    },
    triggers: [triggers.scheduled({ schedule: "*/5 * * * *" })],
  },
});
"#,
        );
        let w = &detect_project(root)[0];
        assert_eq!(w.tool, DevTool::CfCli);
        assert_eq!(w.name.as_deref(), Some("example-worker"));
        let keys: Vec<&str> = w.bindings.iter().map(|b| b.binding.as_str()).collect();
        assert_eq!(
            keys,
            ["DATABASE", "CACHE", "UPLOADS", "MAIL", "ORDERS", "JOBS"]
        );
        assert_eq!(
            w.bindings[0],
            d1("DATABASE", "application-db", Some("9f1c-42"))
        );
        assert_eq!(w.bindings[1].id.as_deref(), Some("kv-1"));
        assert_eq!(w.bindings[2].name.as_deref(), Some("uploads"));
        assert_eq!(w.bindings[4].name.as_deref(), Some("order-workflow"));
        assert_eq!(w.bindings[5].kind, BindingKind::Other("queue".into()));
        assert!(declares_crons(w));

        // A name worked out when it runs isn't guessed; `defineWorker` is read too.
        write(
            &root.join("cloudflare.config.ts"),
            r#"export const api = defineWorker({ name: isStaging ? "a" : "b", env: { DB: bindings.d1() } });"#,
        );
        let w = &detect_project(root)[0];
        assert_eq!(w.name, None);
        assert_eq!(
            w.bindings,
            [Binding {
                kind: BindingKind::D1,
                binding: "DB".into(),
                name: None,
                id: None,
                preview_id: None,
            }]
        );
    }

    /// The Vite plugin marks a project with no config of its own; with a `wrangler.jsonc` it is
    /// still the plugin that serves it, at the version in `node_modules`.
    #[test]
    fn the_vite_plugin_marks_a_worker() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write(
            &root.join("package.json"),
            r#"{"name":"web","scripts":{"dev":"vite","build":"vite build"},"devDependencies":{"@cloudflare/vite-plugin":"^1.50.0","vite":"^7"}}"#,
        );
        write(
            &root.join("node_modules/@cloudflare/vite-plugin/package.json"),
            r#"{"name":"@cloudflare/vite-plugin","version":"1.52.3"}"#,
        );
        let w = &detect_project(root)[0];
        assert_eq!(w.tool, DevTool::VitePlugin);
        assert_eq!(w.config, root.join("package.json"));
        assert_eq!(w.version.as_deref(), Some("1.52.3"));
        assert_eq!(support(w), Support::Ready { cron: false });
        assert_eq!(
            dev_scripts(w),
            [DevScript {
                name: "dev".into(),
                body: "vite".into()
            }]
        );
        write(&root.join("wrangler.jsonc"), r#"{"name":"web"}"#);
        let w = &detect_project(root)[0];
        assert_eq!(
            (w.tool, w.config_name().as_str()),
            (DevTool::VitePlugin, "wrangler.jsonc")
        );
        // A plain Vite app is no Worker.
        let plain = tempfile::tempdir().unwrap();
        write(
            &plain.path().join("package.json"),
            r#"{"devDependencies":{"vite":"^7"}}"#,
        );
        assert!(detect_project(plain.path()).is_empty());
    }

    /// A monorepo: each workspace package with a Worker is one, with Wrangler hoisted to the
    /// root's `node_modules`; a folder beside them with its own config counts as `cf` lays out
    /// several Workers; a package with no Worker doesn't.
    #[test]
    fn a_monorepos_packages_are_each_a_worker() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write(
            &root.join("package.json"),
            r#"{"name":"mono","private":true}"#,
        );
        write(
            &root.join("pnpm-workspace.yaml"),
            "packages:\n  - apps/*\n  - packages/*\n",
        );
        write(&root.join("pnpm-lock.yaml"), "");
        write(
            &root.join("apps/api/package.json"),
            r#"{"name":"@m/api","scripts":{"dev":"wrangler dev --persist-to ../../.state"},"devDependencies":{"wrangler":"^4"}}"#,
        );
        write(
            &root.join("apps/api/wrangler.toml"),
            "name = \"api\"\n[[d1_databases]]\nbinding = \"DB\"\ndatabase_name = \"main\"\ndatabase_id = \"1\"\n",
        );
        write(
            &root.join("apps/site/package.json"),
            r#"{"name":"@m/site","scripts":{"dev":"next dev"}}"#,
        );
        write(
            &root.join("packages/ui/package.json"),
            r#"{"name":"@m/ui"}"#,
        );
        write(
            &root.join("node_modules/wrangler/package.json"),
            r#"{"version":"4.100.2"}"#,
        );
        write(
            &root.join("mail/cloudflare.config.ts"),
            r#"export default defineConfig({ worker: { name: "mail" } });"#,
        );
        let found = detect_project(root);
        let names: Vec<_> = found.iter().map(|w| w.name.as_deref().unwrap()).collect();
        assert_eq!(names, ["api", "mail"]);
        let api = &found[0];
        assert_eq!(api.root, root.join("apps/api"));
        assert_eq!(api.version.as_deref(), Some("4.100.2"));
        assert_eq!(api.persist_to, Some(root.join("apps/api/../../.state")));
        assert_eq!(
            support(api),
            Support::TooOld {
                installed: Version::new(4, 100, 2),
                needed: Version::new(4, 118, 0)
            }
        );
        assert_eq!(upgrade_command(api, root), "pnpm add -D wrangler@latest");
        assert_eq!(dev_scripts(api)[0].name, "dev");
    }

    /// The minimums from Cloudflare's docs: Wrangler 4.118.0 and the Vite plugin 1.50.0 for
    /// Local Explorer, 4.139.0 and 1.60.0 for Cron Triggers; a pre-release comes before its
    /// release, and `cf` or a version not installed says nothing.
    #[test]
    fn versions_are_compared_with_the_minimums() {
        let v = |s: &str| Version::parse(s).unwrap();
        assert!(v("4.118.0") >= v("4.118.0"));
        assert!(v("4.117.9") < v("4.118.0"));
        assert!(v("4.118.0-beta.1") < v("4.118.0"));
        assert!(v("4.118.0-beta.1") > v("4.117.99"));
        assert!(v("4.200") > v("4.139.0"));
        assert_eq!(v("v1.50.0"), Version::new(1, 50, 0));
        assert_eq!(v("1.50.0+build.7"), Version::new(1, 50, 0));
        assert_eq!(Version::parse("latest"), None);
        assert_eq!(Version::parse("1.2.3.4"), None);
        assert_eq!(v("4.118.0").to_string(), "4.118.0");

        let at = |tool: DevTool, version: Option<&str>| {
            support(&WorkersProject {
                root: PathBuf::from("/p"),
                config: PathBuf::from("/p/wrangler.toml"),
                tool,
                version: version.map(Into::into),
                name: None,
                bindings: vec![],
                persist_to: None,
            })
        };
        assert_eq!(
            at(DevTool::Wrangler, Some("4.117.0")),
            Support::TooOld {
                installed: Version::new(4, 117, 0),
                needed: Version::new(4, 118, 0)
            }
        );
        assert_eq!(
            at(DevTool::Wrangler, Some("4.118.0")),
            Support::Ready { cron: false }
        );
        assert_eq!(
            at(DevTool::Wrangler, Some("4.139.0")),
            Support::Ready { cron: true }
        );
        assert_eq!(
            at(DevTool::VitePlugin, Some("1.49.9")),
            Support::TooOld {
                installed: Version::new(1, 49, 9),
                needed: Version::new(1, 50, 0)
            }
        );
        assert_eq!(
            at(DevTool::VitePlugin, Some("1.59.0")),
            Support::Ready { cron: false }
        );
        assert_eq!(
            at(DevTool::VitePlugin, Some("1.60.0")),
            Support::Ready { cron: true }
        );
        assert_eq!(at(DevTool::Wrangler, None), Support::Unknown);
        assert_eq!(at(DevTool::CfCli, Some("1.0.0")), Support::Unknown);
    }

    /// The scripts that start a Workers dev server, and the ones that only build or preview.
    #[test]
    fn dev_server_scripts_are_told_from_builds() {
        let yes = |body: &str| starts_dev_server(body, DevTool::Wrangler);
        assert!(yes("wrangler dev"));
        assert!(yes("wrangler dev --port 8787 --persist-to .state"));
        assert!(yes("npm run build && wrangler dev"));
        assert!(yes("concurrently \"wrangler dev\" \"tsc -w\""));
        assert!(yes("cf dev"));
        assert!(yes("vite"));
        assert!(yes("vite --port 3000"));
        assert!(yes("vite dev"));
        assert!(yes("tsc && vite"));
        assert!(yes("./node_modules/.bin/wrangler dev"));
        assert!(!yes("wrangler deploy"));
        assert!(!yes("vite build"));
        assert!(!yes("vite preview"));
        assert!(!yes("vite build && wrangler deploy"));
        assert!(!yes("react-router dev"));
        assert!(starts_dev_server("react-router dev", DevTool::VitePlugin));
    }

    /// Each package manager's own way to bring a dev dependency up to date.
    #[test]
    fn the_upgrade_uses_the_projects_package_manager() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write(&root.join("wrangler.toml"), "name = \"w\"\n");
        let command = |root: &Path| upgrade_command(&detect_project(root)[0], root);
        assert_eq!(command(root), "npm install -D wrangler@latest");
        write(&root.join("bun.lock"), "");
        assert_eq!(command(root), "bun add -d wrangler@latest");
        write(
            &root.join("package.json"),
            r#"{"packageManager":"yarn@4.1.0","devDependencies":{"@cloudflare/vite-plugin":"1"}}"#,
        );
        assert_eq!(command(root), "yarn add -D @cloudflare/vite-plugin@latest");
        write(&root.join("package.json"), r#"{"packageManager":"pnpm@9"}"#);
        write(&root.join("pnpm-workspace.yaml"), "packages:\n  - apps/*\n");
        assert_eq!(command(root), "pnpm add -D wrangler@latest -w");
    }
}
