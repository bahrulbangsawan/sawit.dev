//! Agent Accounts in the app: the Accounts page (Claude Code's accounts, each its own config
//! folder and sign-in), the title bar's account chip, and the model both follow. It takes over
//! from Claude Pool. Nothing here reads a sign-in: Claude Code's own commands do that.

mod card;
mod chip;
mod list;
mod login;
mod ops;
mod page;
mod rows;
mod settings_tab;
mod statusline_tab;
mod usage_tab;

use std::collections::HashMap;
use std::path::PathBuf;

use gpui_kit::*;
use sawit_core::agent_accounts::claude::{self, AuthStatus};
use sawit_core::agent_accounts::usage::{self, Snapshot};
use sawit_core::agent_accounts::{
    self as aa, Account, Accounts, AgentKind, collector, run, tracking,
};

use crate::rt;

pub use chip::account_chip;
pub use page::{AccountsEvent, AccountsPage};

actions!(
    accounts,
    [
        /// Shows the Accounts page: Claude Code's accounts, their sign-in and usage.
        OpenAccounts
    ]
);

type AnalyticsRows = (
    Vec<(
        String,
        String,
        sawit_core::agent_accounts::analytics::Totals,
    )>,
    Vec<(
        String,
        String,
        sawit_core::agent_accounts::analytics::Totals,
    )>,
);

/// How often the usage files are read again (they are tiny).
const USAGE_EVERY: std::time::Duration = std::time::Duration::from_secs(5);

/// What the app knows of one account's sign-in.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SignIn {
    Unknown,
    Checking,
    Known(AuthStatus),
    Failed(String),
}

/// The accounts, their sign-in and usage, shared by every window.
pub struct AccountsModel {
    pub root: PathBuf,
    pub home: PathBuf,
    pub accounts: Accounts,
    pub sign_in: HashMap<String, SignIn>,
    pub usage: HashMap<String, Snapshot>,
    pub plan: HashMap<String, String>,
    /// What went wrong last (shown on the page until the next action).
    pub error: Option<String>,
    /// What the last action did.
    pub notice: Option<String>,
    /// The analytics: per account and model, the period and the one before.
    pub analytics: Option<AnalyticsRows>,
    /// Limit alerts sent (account·window·reset → level), and whether the values at launch were
    /// recorded yet (they never alert).
    pub alerts_sent: std::collections::BTreeMap<String, u8>,
    pub alerts_primed: bool,
    /// What the last shared-settings run changed, and when (Unix seconds).
    pub shared: Option<(i64, Vec<String>)>,
    /// The account whose usage check runs.
    pub checking_usage: Option<String>,
    pub login: Option<login::LoginState>,
    /// The `claude` Sawit runs; none in tests unless a test gives a stand-in.
    pub claude: Option<PathBuf>,
    _poll: Task<()>,
}

struct GlobalAccounts(Entity<AccountsModel>);

impl Global for GlobalAccounts {}

/// The model, made on first use.
pub fn global(cx: &mut App) -> Entity<AccountsModel> {
    if let Some(g) = cx.try_global::<GlobalAccounts>() {
        return g.0.clone();
    }
    let root = sawit_core::instances::AppPaths::default_macos().root;
    let home = PathBuf::from(std::env::var("HOME").unwrap_or_default());
    let model = cx.new(|cx| AccountsModel::new(root, home, cx));
    cx.set_global(GlobalAccounts(model.clone()));
    model
}

/// At launch: the collector's link points at this app, and the model loads.
pub fn init(cx: &mut App) {
    cx.bind_keys([KeyBinding::new("cmd-alt-a", OpenAccounts, None)]);
    let model = global(cx);
    let root = model.read(cx).root.clone();
    if let Ok(exe) = std::env::current_exe() {
        let _ = collector::install_link(&root, &exe);
    }
    model.update(cx, |m, cx| m.check_all(cx));
}

impl AccountsModel {
    pub(crate) fn new(root: PathBuf, home: PathBuf, cx: &mut Context<Self>) -> Self {
        let claude = if cfg!(test) {
            None
        } else {
            run::find_claude(&home)
        };
        let mut this = AccountsModel {
            claude,
            root,
            home,
            accounts: Accounts::default(),
            sign_in: HashMap::new(),
            usage: HashMap::new(),
            plan: HashMap::new(),
            error: None,
            notice: None,
            checking_usage: None,
            shared: None,
            analytics: None,
            alerts_sent: Default::default(),
            alerts_primed: false,
            login: None,
            _poll: Task::ready(()),
        };
        this.reload();
        this.seed_default();
        this._poll = cx.spawn(async move |this, cx| {
            let mut ticks = 0u32;
            loop {
                cx.background_executor().timer(USAGE_EVERY).await;
                ticks += 1;
                // Shared settings once a minute, to catch `claude mcp add` or `/plugin install`
                // inside a session.
                let sync = ticks.is_multiple_of(12);
                let alive = this.update(cx, |m, cx| {
                    m.read_usage(cx);
                    if sync {
                        m.sync_shared(cx);
                    }
                });
                if alive.is_err() {
                    return;
                }
            }
        });
        this
    }

    /// Reads the accounts and their usage from disk.
    pub fn reload(&mut self) {
        match Accounts::load(&self.root) {
            Ok(a) => self.accounts = a,
            Err(e) => self.error = Some(e.to_string()),
        }
        self.usage = self
            .accounts
            .accounts
            .iter()
            .filter_map(|a| Some((a.id.clone(), usage::load_snapshot(&self.root, &a.id)?)))
            .collect();
    }

    /// With no accounts yet, the existing `~/.claude` is the default one (nothing is written to it).
    fn seed_default(&mut self) {
        if !self.accounts.accounts.is_empty() {
            return;
        }
        let dir = AgentKind::ClaudeCode.default_dir(&self.home);
        if dir.is_dir() {
            let _ = self.accounts.add(new_account("default", dir, true));
            self.save();
        }
    }

    fn read_usage(&mut self, cx: &mut Context<Self>) {
        // `sawit account` and the import change the file too: read it again when it did.
        if let Ok(on_disk) = Accounts::load(&self.root)
            && on_disk != self.accounts
        {
            self.accounts = on_disk;
            cx.notify();
        }
        let before = self.usage.clone();
        for a in &self.accounts.accounts {
            if let Some(s) = usage::load_snapshot(&self.root, &a.id) {
                self.usage.insert(a.id.clone(), s);
            }
        }
        if self.usage != before {
            self.check_alerts(cx);
            cx.notify();
        }
    }

    pub fn save(&mut self) {
        if let Err(e) = self.accounts.save(&self.root) {
            self.error = Some(format!("Couldn't save the accounts: {e}"));
        }
    }

    pub fn list(&self) -> Vec<&Account> {
        self.accounts.of(AgentKind::ClaudeCode)
    }

    pub fn active(&self) -> Option<&Account> {
        self.accounts.active(AgentKind::ClaudeCode, &self.root)
    }

    /// Checks every account's sign-in with `claude auth status`, and reads its plan.
    pub fn check_all(&mut self, cx: &mut Context<Self>) {
        let ids: Vec<String> = self.list().iter().map(|a| a.id.clone()).collect();
        for id in ids {
            self.check(&id, cx);
        }
    }

    pub fn check(&mut self, id: &str, cx: &mut Context<Self>) {
        let Some(account) = self.accounts.get(id).cloned() else {
            return;
        };
        let home = self.home.clone();
        let Some(bin) = self.claude.clone() else {
            self.sign_in.insert(
                id.into(),
                SignIn::Failed("Claude Code isn't installed (no `claude` on PATH).".into()),
            );
            cx.notify();
            return;
        };
        self.sign_in.insert(id.into(), SignIn::Checking);
        cx.notify();
        let rx = rt::spawn_blocking(cx, move || {
            let status = run::auth_status(&bin, &account, &home);
            let details = std::fs::read_to_string(claude::global_config(&account, &home))
                .ok()
                .and_then(|t| claude::parse_details(&t));
            (status, details)
        });
        let id = id.to_string();
        cx.spawn(async move |this, cx| {
            let Ok((status, details)) = rx.await else {
                return;
            };
            let _ = this.update(cx, |m, cx| {
                match status {
                    Ok(s) => {
                        if let Some(p) = claude::plan(
                            s.subscription_type.as_deref(),
                            details.as_ref(),
                            s.email.as_deref(),
                        ) {
                            m.plan.insert(id.clone(), p);
                        }
                        // The status line shows who the account is: kept current.
                        let sl = sawit_core::agent_accounts::statusline::load_account(&m.root, &id);
                        if let Some(mut line) = sl.filter(|l| l.managed) {
                            line.account = s.email.clone();
                            line.plan = m.plan.get(&id).cloned();
                            let _ = sawit_core::agent_accounts::statusline::save_account(
                                &m.root, &id, &line,
                            );
                        }
                        m.sign_in.insert(id, SignIn::Known(s));
                    }
                    Err(e) => {
                        m.sign_in.insert(id, SignIn::Failed(e.to_string()));
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub fn status(&self, id: &str) -> Option<&AuthStatus> {
        match self.sign_in.get(id) {
            Some(SignIn::Known(s)) => Some(s),
            _ => None,
        }
    }

    /// Makes `id` the account new `claude` launches use (the shell block and `sawit account`
    /// read the pointer). Running sessions keep theirs.
    pub fn set_active(&mut self, id: &str, cx: &mut Context<Self>) {
        let Some(a) = self.accounts.get(id).cloned() else {
            return;
        };
        self.sync_shared(cx);
        match aa::write_active(&self.root, &a) {
            Ok(()) => self.notice = Some(format!("New Claude Code sessions use {}.", a.name)),
            Err(e) => self.error = Some(e.to_string()),
        }
        cx.notify();
    }

    /// Adds an account: a new folder `~/.claude-<name>` (0700) unless `dir` names one.
    pub fn add(
        &mut self,
        name: &str,
        dir: Option<PathBuf>,
        cx: &mut Context<Self>,
    ) -> Option<String> {
        let name = aa::kebab(name);
        let dir = dir.unwrap_or_else(|| {
            self.home.join(format!(
                ".claude-{}",
                if name.is_empty() { "account" } else { &name }
            ))
        });
        if !dir.exists() {
            use std::os::unix::fs::DirBuilderExt as _;
            if let Err(e) = std::fs::DirBuilder::new()
                .recursive(true)
                .mode(0o700)
                .create(&dir)
            {
                self.error = Some(format!("Couldn't create {}: {e}", dir.display()));
                cx.notify();
                return None;
            }
        }
        let is_default = dir == AgentKind::ClaudeCode.default_dir(&self.home);
        let id = match self.accounts.add(new_account(&name, dir, is_default)) {
            Ok(a) => a.id.clone(),
            Err(e) => {
                self.error = Some(e.to_string());
                cx.notify();
                return None;
            }
        };
        self.save();
        self.check(&id, cx);
        self.sync_shared(cx);
        cx.notify();
        Some(id)
    }

    pub fn rename(&mut self, id: &str, name: &str, cx: &mut Context<Self>) {
        match self.accounts.rename(id, name) {
            Ok(_) => self.save(),
            Err(e) => self.error = Some(e.to_string()),
        }
        cx.notify();
    }

    /// Takes the account out of Sawit: tracking off first (its `statusLine` given back); the
    /// folder and sign-in stay.
    pub fn remove(&mut self, id: &str, cx: &mut Context<Self>) {
        if let Some(mut a) = self.accounts.get(id).cloned() {
            if a.tracking {
                let _ = tracking::disable(&self.root, &mut a, sawit_core::github::now());
            }
            self.accounts.remove(id);
            self.save();
            self.notice = Some(format!(
                "Removed {} from Sawit. {} and its sign-in are still there.",
                a.name,
                a.dir.display()
            ));
        }
        cx.notify();
    }

    /// What `app_state` and agents see: names, active, sign-in and usage, never a secret.
    pub fn describe(&self) -> serde_json::Value {
        let active = self.active().map(|a| a.id.clone());
        serde_json::Value::Array(
            self.list()
                .iter()
                .map(|a| {
                    let s = self.status(&a.id);
                    let u = self.usage.get(&a.id);
                    serde_json::json!({
                        "name": a.name,
                        "dir": a.dir,
                        "default": a.is_default,
                        "active": Some(&a.id) == active.as_ref(),
                        "signed_in": s.map(|s| s.signed_in),
                        "email": s.and_then(|s| s.email.clone()),
                        "plan": self.plan.get(&a.id),
                        "five_hour": u.and_then(|u| u.five_hour).map(|w| w.used_percentage),
                        "seven_day": u.and_then(|u| u.seven_day).map(|w| w.used_percentage),
                    })
                })
                .collect(),
        )
    }
}

/// A new account record (folder and colour by order).
pub(crate) fn new_account(name: &str, dir: PathBuf, is_default: bool) -> Account {
    const COLORS: &[&str] = &[
        "#D97757", "#4F8EF7", "#3DBE8B", "#B67CF2", "#F2B84B", "#E5658A",
    ];
    Account {
        id: aa::new_id(),
        kind: AgentKind::ClaudeCode,
        name: name.into(),
        color: COLORS[aa::kebab(name).len() % COLORS.len()].into(),
        dir,
        is_default,
        order: 0,
        tracking: false,
        original_status_line: None,
        created_at: sawit_core::github::now(),
    }
}

#[cfg(test)]
mod tests;
