//! Where the accounts are kept: `<Sawit data>/accounts/accounts.json` (0600), the settings beside
//! it, and the active account per agent in `active-<agent>` (its folder; empty for the default),
//! which the shell block and `sawit account` read when they start the agent.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::{Account, AgentKind, kebab};
use crate::{CoreError, Result};

/// The accounts folder in Sawit's data folder `root`.
pub fn dir(root: &Path) -> PathBuf {
    root.join("accounts")
}

/// The settings the Accounts page keeps.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Shared Settings: MCP servers, skills, plugins, hooks and permissions the same in every
    /// account.
    pub share_extensions: bool,
    /// One notification per window near and at a limit.
    pub limit_alerts: bool,
    /// The percentage the alert and the suggestion start at.
    pub limit_threshold: u8,
    /// While the active account is near a limit, name the one with the most room.
    pub suggest_account: bool,
    /// What the usage collector prints when an account had no status line of its own:
    /// `summary` (`5h 23% · 7d 41%`) or `none`.
    pub collector_output: String,
    /// Open in Terminal asks for a project in this folder first, when set.
    pub projects_folder: Option<PathBuf>,
    /// Claude Pool's data was imported (the offer doesn't show again).
    pub imported_from_pool: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            share_extensions: false,
            limit_alerts: false,
            limit_threshold: 90,
            suggest_account: false,
            collector_output: "summary".into(),
            projects_folder: None,
            imported_from_pool: false,
        }
    }
}

/// Every account, in the user's order.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Accounts {
    pub accounts: Vec<Account>,
    #[serde(default)]
    pub settings: Settings,
}

impl Accounts {
    pub fn path(root: &Path) -> PathBuf {
        dir(root).join("accounts.json")
    }

    /// The saved accounts; none yet is an empty list. A file that isn't valid is an error, so it
    /// is never saved over.
    pub fn load(root: &Path) -> Result<Accounts> {
        match std::fs::read_to_string(Self::path(root)) {
            Ok(text) => serde_json::from_str(&text)
                .map_err(|e| CoreError::Parse(format!("accounts.json: {e}"))),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Accounts::default()),
            Err(e) => Err(e.into()),
        }
    }

    pub fn save(&self, root: &Path) -> Result<()> {
        let path = Self::path(root);
        private_dir(&dir(root))?;
        let text =
            serde_json::to_string_pretty(self).map_err(|e| CoreError::Parse(e.to_string()))?;
        write_private(&path, text.as_bytes())
    }

    /// The accounts of `kind`, in order.
    pub fn of(&self, kind: AgentKind) -> Vec<&Account> {
        let mut list: Vec<&Account> = self.accounts.iter().filter(|a| a.kind == kind).collect();
        list.sort_by_key(|a| a.order);
        list
    }

    pub fn get(&self, id: &str) -> Option<&Account> {
        self.accounts.iter().find(|a| a.id == id)
    }

    /// An account by what a person types: its name, its number in the list (1-based), or the
    /// start of exactly one name.
    pub fn find(&self, kind: AgentKind, what: &str) -> Option<&Account> {
        let list = self.of(kind);
        if let Ok(n) = what.parse::<usize>()
            && n >= 1
            && let Some(a) = list.get(n - 1)
        {
            return Some(a);
        }
        let want = kebab(what);
        if let Some(a) = list.iter().find(|a| a.name == want) {
            return Some(a);
        }
        let mut starts = list
            .iter()
            .filter(|a| !want.is_empty() && a.name.starts_with(&want));
        match (starts.next(), starts.next()) {
            (Some(a), None) => Some(a),
            _ => None,
        }
    }

    /// Adds `account` last, its name made kebab-case and unique (`work-2`), its folder unused.
    pub fn add(&mut self, mut account: Account) -> Result<&Account> {
        if self
            .accounts
            .iter()
            .any(|a| a.kind == account.kind && a.dir == account.dir)
        {
            return Err(CoreError::Config(format!(
                "{} is already an account.",
                account.dir.display()
            )));
        }
        if account.is_default && self.of(account.kind).iter().any(|a| a.is_default) {
            account.is_default = false;
        }
        account.name = self.unique_name(account.kind, &account.name, None);
        account.order = self.of(account.kind).len();
        self.accounts.push(account);
        Ok(self.accounts.last().expect("just pushed"))
    }

    /// `name` in kebab-case, with `-2`, `-3`… when another account of `kind` has it.
    pub fn unique_name(&self, kind: AgentKind, name: &str, except: Option<&str>) -> String {
        let base = match kebab(name) {
            n if n.is_empty() => "account".to_string(),
            n => n,
        };
        let taken = |n: &str| {
            self.accounts
                .iter()
                .any(|a| a.kind == kind && a.name == n && Some(a.id.as_str()) != except)
        };
        if !taken(&base) {
            return base;
        }
        (2..)
            .map(|i| format!("{base}-{i}"))
            .find(|n| !taken(n))
            .expect("a free name")
    }

    pub fn rename(&mut self, id: &str, name: &str) -> Result<String> {
        let kind = self
            .get(id)
            .ok_or_else(|| CoreError::Config("No such account.".into()))?
            .kind;
        let name = self.unique_name(kind, name, Some(id));
        if let Some(a) = self.accounts.iter_mut().find(|a| a.id == id) {
            a.name = name.clone();
        }
        Ok(name)
    }

    /// Takes the account out of the list; its folder and sign-in stay.
    pub fn remove(&mut self, id: &str) -> Option<Account> {
        let ix = self.accounts.iter().position(|a| a.id == id)?;
        let gone = self.accounts.remove(ix);
        for (order, a) in self
            .accounts
            .iter_mut()
            .filter(|a| a.kind == gone.kind)
            .enumerate()
        {
            a.order = order;
        }
        Some(gone)
    }

    /// The account the agent starts with now: the one the pointer names, else the default.
    pub fn active(&self, kind: AgentKind, root: &Path) -> Option<&Account> {
        let pointer = read_active(root, kind);
        let list = self.of(kind);
        match pointer {
            Some(dir) => list.into_iter().find(|a| !a.is_default && a.dir == dir),
            None => list.into_iter().find(|a| a.is_default),
        }
    }
}

/// The file naming the active account of `kind`.
pub fn active_path(root: &Path, kind: AgentKind) -> PathBuf {
    dir(root).join(format!("active-{}", kind.slug()))
}

/// The active account's folder; `None` for the default (an empty or missing file).
pub fn read_active(root: &Path, kind: AgentKind) -> Option<PathBuf> {
    let text = std::fs::read_to_string(active_path(root, kind)).ok()?;
    let text = text.trim();
    (!text.is_empty()).then(|| PathBuf::from(text))
}

/// Makes `account` the active one: its folder in the pointer, or nothing for the default.
pub fn write_active(root: &Path, account: &Account) -> Result<()> {
    private_dir(&dir(root))?;
    let text = account
        .env_value()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_default();
    write_private(&active_path(root, account.kind), text.as_bytes())
}

/// A folder only this user can open.
pub(crate) fn private_dir(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt as _;
    std::fs::create_dir_all(path)?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))?;
    Ok(())
}

/// Writes `bytes` atomically, readable by this user only.
pub(crate) fn write_private(path: &Path, bytes: &[u8]) -> Result<()> {
    crate::files::replace_file(path, bytes, {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::Permissions::from_mode(0o600)
    })
}
