//! Taking over from Claude Pool: its accounts, settings, usage history, status line layout and
//! shared-settings state come into Sawit (copied, never moved), each account's `statusLine` is
//! pointed from Claude Pool's collector to Sawit's, and its shell block and `cp1`…`cp9` links
//! are replaced by Sawit's. Everything is listed first ([`plan`]) and backed up before it changes.
//!
//! Then, only when the user confirms, Claude Pool itself can go ([`removal`]): `~/.claude-pool`
//! and its app data are packed into one archive in Sawit's data folder, the app goes to the
//! Trash, and its command-line links are removed. Account folders and sign-ins are never touched.

use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde_json::Value;

use super::store::{private_dir, write_private};
use super::{Account, Accounts, AgentKind, collector, kebab, shell, statusline, tracking, usage};
use crate::{CoreError, Result};

/// Where Claude Pool keeps things, under `home`.
pub struct PoolPaths {
    pub home_dir: PathBuf,
    pub support: PathBuf,
    pub caches: PathBuf,
    pub app: PathBuf,
}

impl PoolPaths {
    /// Claude Pool's places for the user whose home is `home`. The Mac-wide ones (the app in
    /// /Applications) count only when `home` is this user's own home folder: a Sawit run with
    /// another HOME (a test's) sees only what is in that folder.
    pub fn of(home: &Path) -> PoolPaths {
        let app = if is_users_home(home) {
            PathBuf::from("/Applications/ClaudePool.app")
        } else {
            home.join("Applications/ClaudePool.app")
        };
        PoolPaths {
            home_dir: home.join(".claude-pool"),
            support: home.join("Library/Application Support/ClaudePool"),
            caches: home.join("Library/Caches/ClaudePool"),
            app,
        }
    }
}

/// Whether `home` is this user's own home folder (the password database's), not one a test or
/// another run put in HOME.
pub fn is_users_home(home: &Path) -> bool {
    let pw = unsafe { libc::getpwuid(libc::getuid()) };
    if pw.is_null() {
        return false;
    }
    let dir = unsafe { std::ffi::CStr::from_ptr((*pw).pw_dir) };
    let real = PathBuf::from(dir.to_string_lossy().to_string());
    let canon = |p: &Path| std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    canon(&real) == canon(home)
}

/// Claude Pool's data to import, when there is some.
pub fn found(home: &Path) -> Option<PathBuf> {
    let p = PoolPaths::of(home).support.join("profiles.json");
    p.is_file().then_some(p)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PoolProfile {
    id: String,
    name: String,
    config_dir: String,
    #[serde(default)]
    color_hex: Option<String>,
    #[serde(default)]
    is_default: bool,
    #[serde(default)]
    tracking_enabled: bool,
    #[serde(default)]
    original_status_line: Option<String>,
    #[serde(default)]
    sort_index: usize,
    #[serde(default)]
    created_at: Option<String>,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct PoolSettings {
    share_extensions: Option<bool>,
    limit_alerts_enabled: Option<bool>,
    limit_alert_threshold: Option<u8>,
    suggest_account_enabled: Option<bool>,
    collector_output: Option<String>,
    projects_folder: Option<String>,
}

/// A `statusLine` to point from Claude Pool's collector to Sawit's.
#[derive(Clone, Debug, PartialEq)]
pub struct Repoint {
    pub account: String,
    pub settings: PathBuf,
    pub before: Value,
    pub after: Value,
}

/// What an import does, worked out before anything changes.
#[derive(Clone, Debug, PartialEq)]
pub struct Plan {
    pub accounts: Vec<Account>,
    pub settings: super::Settings,
    /// Files copied into Sawit's data folder: (from, to).
    pub copies: Vec<(PathBuf, PathBuf)>,
    pub repoints: Vec<Repoint>,
    /// The pointer's folder, when an account other than the default is active.
    pub active: Option<PathBuf>,
    /// The startup file whose Claude Pool block is replaced.
    pub shell_file: Option<PathBuf>,
}

impl Plan {
    /// The lines the confirmation lists.
    pub fn summary(&self) -> Vec<String> {
        let mut out = vec![format!(
            "{} accounts: {}",
            self.accounts.len(),
            self.accounts
                .iter()
                .map(|a| a.name.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        )];
        out.push(format!("{} files copied (usage history, status line layout, shared-settings state); Claude Pool's own stay as they are", self.copies.len()));
        for r in &self.repoints {
            out.push(format!(
                "{}: statusLine from Claude Pool's collector to Sawit's (backed up first)",
                r.settings.display()
            ));
        }
        if let Some(f) = &self.shell_file {
            out.push(format!(
                "{}: Claude Pool's claude() block replaced by Sawit's (backed up first)",
                f.display()
            ));
        }
        out
    }
}

/// Reads Claude Pool's files and works out the import; writes nothing.
pub fn plan(home: &Path, root: &Path, login_shell: &str) -> Result<Plan> {
    let pool = PoolPaths::of(home);
    let text = std::fs::read_to_string(pool.support.join("profiles.json"))?;
    let profiles: Vec<PoolProfile> = serde_json::from_str(&text)
        .map_err(|e| CoreError::Parse(format!("Claude Pool's profiles.json: {e}")))?;
    let ps: PoolSettings = std::fs::read_to_string(pool.support.join("settings.json"))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default();
    let mut profiles = profiles;
    profiles.sort_by_key(|p| p.sort_index);
    let expand = |p: &str| match p.strip_prefix("~/") {
        Some(rest) => home.join(rest),
        None => PathBuf::from(p),
    };
    let accounts: Vec<Account> = profiles
        .iter()
        .enumerate()
        .map(|(order, p)| Account {
            id: p.id.clone(),
            kind: AgentKind::ClaudeCode,
            name: match kebab(&p.name) {
                n if n.is_empty() => format!("account-{}", order + 1),
                n => n,
            },
            dir: expand(&p.config_dir),
            is_default: p.is_default,
            color: p
                .color_hex
                .clone()
                .map(|c| {
                    if c.starts_with('#') {
                        c
                    } else {
                        format!("#{c}")
                    }
                })
                .unwrap_or_else(|| "#D97757".into()),
            order,
            tracking: p.tracking_enabled,
            original_status_line: p
                .original_status_line
                .as_deref()
                .and_then(|s| serde_json::from_str(s).ok()),
            created_at: p
                .created_at
                .as_deref()
                .and_then(crate::github::parse_time)
                .unwrap_or(0),
        })
        .collect();
    let defaults = super::Settings::default();
    let settings = super::Settings {
        share_extensions: ps.share_extensions.unwrap_or(defaults.share_extensions),
        limit_alerts: ps.limit_alerts_enabled.unwrap_or(false),
        limit_threshold: ps.limit_alert_threshold.unwrap_or(defaults.limit_threshold),
        suggest_account: ps.suggest_account_enabled.unwrap_or(false),
        collector_output: ps.collector_output.unwrap_or(defaults.collector_output),
        projects_folder: ps.projects_folder.as_deref().map(expand),
        imported_from_pool: true,
    };
    let mut copies = Vec::new();
    let into = super::store::dir(root);
    for a in &accounts {
        for suffix in ["json", "history.jsonl", "reporters.json"] {
            let from = pool
                .home_dir
                .join("usage")
                .join(format!("{}.{suffix}", a.id));
            if from.is_file() {
                copies.push((
                    from,
                    usage::usage_dir(root).join(format!("{}.{suffix}", a.id)),
                ));
            }
        }
        let line = pool
            .home_dir
            .join("statusline")
            .join(format!("profile-{}.json", a.id));
        if line.is_file() {
            copies.push((line, statusline::account_path(root, &a.id)));
        }
    }
    let layout = pool.home_dir.join("statusline/config.json");
    if layout.is_file() {
        copies.push((layout, statusline::config_path(root)));
    }
    let state = pool.home_dir.join("shared-sync-state.json");
    if state.is_file() {
        copies.push((state, into.join("shared-sync-state.json")));
    }
    let mut repoints = Vec::new();
    for a in &accounts {
        let path = tracking::settings_path(a);
        let Ok(s) = tracking::read_settings(&path) else {
            continue;
        };
        let Some(before) = s.get("statusLine").cloned() else {
            continue;
        };
        let cmd = before
            .get("command")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if !cmd.contains("claude-pool-collector") {
            continue;
        }
        let mut after = before.clone();
        after["command"] = Value::String(collector::command(root, &a.id));
        repoints.push(Repoint {
            account: a.id.clone(),
            settings: path,
            before,
            after,
        });
    }
    let active = std::fs::read_to_string(pool.home_dir.join("active"))
        .ok()
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
        .map(PathBuf::from);
    let shell_file = shell::target(home, login_shell)
        .filter(|f| std::fs::read_to_string(f).is_ok_and(|t| shell::has_pool_block(&t)));
    Ok(Plan {
        accounts,
        settings,
        copies,
        repoints,
        active,
        shell_file,
    })
}

/// Carries the plan out. Each changed file is backed up first; Claude Pool's files are copied,
/// not moved. Returns what it did.
pub fn apply(plan: &Plan, root: &Path, now: i64) -> Result<Vec<String>> {
    let mut done = Vec::new();
    let mut all = Accounts::load(root).unwrap_or_default();
    // An account Sawit listed by itself for the same folder gives way to Claude Pool's.
    all.accounts
        .retain(|a| !plan.accounts.iter().any(|p| p.dir == a.dir));
    all.accounts.extend(plan.accounts.iter().cloned());
    all.settings = plan.settings.clone();
    all.save(root)?;
    done.push(format!("{} accounts imported", plan.accounts.len()));
    for (from, to) in &plan.copies {
        if let Some(d) = to.parent() {
            private_dir(d)?;
        }
        write_private(to, &std::fs::read(from)?)?;
    }
    done.push(format!("{} files copied", plan.copies.len()));
    if let Some(active) = &plan.active
        && let Some(a) = plan.accounts.iter().find(|a| &a.dir == active)
    {
        super::write_active(root, a)?;
    }
    for r in &plan.repoints {
        let mut s = tracking::read_settings(&r.settings)?;
        tracking::backup(root, &r.account, &r.settings, now)?;
        s.insert("statusLine".into(), r.after.clone());
        let text = serde_json::to_string_pretty(&s).map_err(|e| CoreError::Parse(e.to_string()))?;
        crate::files::write_atomic(&r.settings, format!("{text}\n").as_bytes())?;
        done.push(format!(
            "{}: statusLine now runs Sawit",
            r.settings.display()
        ));
    }
    if let Some(f) = &plan.shell_file {
        let text = std::fs::read_to_string(f)?;
        let new = shell::with_block(&text, root)?;
        shell::write(root, f, &new, now)?;
        done.push(format!(
            "{}: Sawit's block in place of Claude Pool's",
            f.display()
        ));
    }
    Ok(done)
}

/// What removing Claude Pool would do, and what still stops it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Removal {
    /// Folders packed into the archive, then removed.
    pub folders: Vec<PathBuf>,
    /// The app, to the Trash.
    pub app: Option<PathBuf>,
    /// Links to Claude Pool's app, removed.
    pub links: Vec<PathBuf>,
    pub archive: PathBuf,
    /// Why it can't go yet; empty when it can.
    pub blockers: Vec<String>,
}

/// Where Claude Pool's command-line links can be: `~/.local/bin`, `/opt/homebrew/bin`,
/// `/usr/local/bin`.
/// The Mac-wide ones only when `home` is this user's own ([`is_users_home`]).
pub fn link_dirs(home: &Path) -> Vec<PathBuf> {
    let mut dirs = vec![home.join(".local/bin")];
    if is_users_home(home) {
        dirs.extend([
            PathBuf::from("/opt/homebrew/bin"),
            PathBuf::from("/usr/local/bin"),
        ]);
    }
    dirs
}

/// The removal, with what blocks it: Claude Pool running, a `statusLine` still running its
/// collector, its shell block still there. `pool` and `link_dirs` say where to look (the real
/// places in the app, a temp folder in tests).
#[allow(clippy::too_many_arguments)]
pub fn removal(
    home: &Path,
    pool: &PoolPaths,
    link_dirs: &[PathBuf],
    root: &Path,
    accounts: &[&Account],
    login_shell: &str,
    running: bool,
    now: i64,
) -> Removal {
    let folders: Vec<PathBuf> = [
        pool.home_dir.clone(),
        pool.support.clone(),
        pool.caches.clone(),
    ]
    .into_iter()
    .filter(|p| p.exists())
    .collect();
    let mut links = Vec::new();
    for dir in link_dirs {
        for name in
            std::iter::once("claude-pool".to_string()).chain((1..=9).map(|n| format!("cp{n}")))
        {
            let p = dir.join(&name);
            if std::fs::read_link(&p).is_ok_and(|t| t.to_string_lossy().contains("ClaudePool.app"))
            {
                links.push(p);
            }
        }
    }
    let mut blockers = Vec::new();
    if running {
        blockers.push("Claude Pool is running: quit it first (its menu › Quit).".into());
    }
    for a in accounts {
        let still = tracking::read_settings(&tracking::settings_path(a))
            .ok()
            .and_then(|s| {
                s.get("statusLine")
                    .and_then(|v| v.get("command"))
                    .and_then(Value::as_str)
                    .map(|c| c.contains("claude-pool"))
            })
            .unwrap_or(false);
        if still {
            blockers.push(format!(
                "{}'s statusLine still runs Claude Pool's collector: import first.",
                a.name
            ));
        }
    }
    if shell::target(home, login_shell)
        .is_some_and(|f| std::fs::read_to_string(f).is_ok_and(|t| shell::has_pool_block(&t)))
    {
        blockers.push("Your shell startup file still has Claude Pool's block: install Sawit's in its place first.".into());
    }
    let stamp = crate::github::stamp(now).replace(':', "");
    Removal {
        folders,
        app: Some(pool.app.clone()).filter(|a| a.exists()),
        links,
        archive: root
            .join("backups")
            .join(format!("claude-pool-{stamp}.tar.gz")),
        blockers,
    }
}

/// Packs the folders into the archive (0600) and checks it reads back, then removes them and the
/// links. The app is left to the caller, which moves it to the Trash. Nothing is removed unless
/// the archive was written and verified.
pub fn remove(r: &Removal, home: &Path) -> Result<()> {
    if !r.blockers.is_empty() {
        return Err(CoreError::Config(r.blockers.join(" ")));
    }
    if let Some(d) = r.archive.parent() {
        private_dir(d)?;
    }
    if !r.folders.is_empty() {
        let rel: Vec<String> = r
            .folders
            .iter()
            .map(|f| {
                f.strip_prefix(home)
                    .unwrap_or(f)
                    .to_string_lossy()
                    .to_string()
            })
            .collect();
        let out = std::process::Command::new("/usr/bin/tar")
            .arg("-czf")
            .arg(&r.archive)
            .arg("-C")
            .arg(home)
            .args(&rel)
            .output()?;
        if !out.status.success() {
            return Err(CoreError::Config(format!(
                "The backup couldn't be written: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            )));
        }
        {
            use std::os::unix::fs::PermissionsExt as _;
            std::fs::set_permissions(&r.archive, std::fs::Permissions::from_mode(0o600))?;
        }
        let list = std::process::Command::new("/usr/bin/tar")
            .arg("-tzf")
            .arg(&r.archive)
            .output()?;
        if !list.status.success() || list.stdout.is_empty() {
            return Err(CoreError::Config(
                "The backup didn't read back, so nothing was removed.".into(),
            ));
        }
    }
    for f in &r.folders {
        std::fs::remove_dir_all(f)?;
    }
    for l in &r.links {
        std::fs::remove_file(l)?;
    }
    Ok(())
}
