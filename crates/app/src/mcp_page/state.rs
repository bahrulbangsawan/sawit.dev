//! What the page knows: the clients, configs and programs a detection finds (off the UI
//! thread), the statuses, Region and sign-ins worked out from them, and what Sawit's GitHub
//! sign-in allows.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use gpui_kit::*;
use sawit_core::dev_tools::find_program;
use sawit_core::mcp_auth::{SignIn, sign_in_states};
use sawit_core::mcp_catalog::{Auth, DEFAULT_AWS_REGION, GITHUB_SCOPES, SERVERS};
use sawit_core::mcp_clients::{Client, Entries, Spec, Status, discover, read_entries, status_in};
use sawit_core::package_manager::{self, PackageManager, Use};

use super::{Detected, Filter, McpPage, PROGRAMS, Pair, Sheet, Sort};
use crate::git::account;
use crate::{catalog, rt};

impl McpPage {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let mut this = Self::blank(cx);
        this.make_search(window, cx);
        this.reload(cx);
        this
    }

    /// The page before anything is detected.
    pub(super) fn blank(cx: &mut Context<Self>) -> Self {
        let server = std::env::current_exe()
            .ok()
            .and_then(|exe| exe.parent().map(|d| d.join("sawit-mcp")))
            .unwrap_or_else(|| PathBuf::from("sawit-mcp"));
        let github = account::global(cx);
        // GitHub's Install turns on and off with the one sign-in.
        let github_sub = cx.observe(&github, |_, _, cx| cx.notify());
        Self {
            github,
            _github: github_sub,
            server_exists: false,
            server,
            targets: Vec::new(),
            entries: Vec::new(),
            status: HashMap::new(),
            sign_ins: HashMap::new(),
            checking: HashSet::new(),
            clis: HashMap::new(),
            programs: HashMap::new(),
            region: DEFAULT_AWS_REGION.into(),
            region_chosen: false,
            busy: HashSet::new(),
            stops: HashMap::new(),
            errors: HashMap::new(),
            loading: true,
            sheet: None,
            focus: cx.focus_handle(),
            filter: Filter::All,
            kind: None,
            sort: Sort::Catalog,
            paging: catalog::Paging::new("mcp", cx),
            cursor: None,
            last_query: String::new(),
            page_focus: cx.focus_handle(),
            refocus: false,
            search: None,
            _search: None,
            pms: Vec::new(),
            pm: None,
        }
    }

    /// The package manager that runs packages: the one picked, npm's npx until
    /// one is.
    pub(super) fn run_pm(&self) -> PackageManager {
        self.pm.unwrap_or(PackageManager::Npm)
    }

    pub(super) fn spec(&self, ix: usize) -> Spec {
        let programs = self.programs.clone();
        SERVERS[ix].spec_with(
            &self.server,
            &self.region,
            move |c| programs.get(c).cloned(),
            self.run_pm(),
        )
    }

    /// Whether server `s` runs through a package manager: a local server's
    /// package, or the bridge some of the clients here need.
    pub(super) fn uses_runner(&self, s: usize) -> bool {
        SERVERS[s].package().is_some()
            || self
                .targets
                .iter()
                .any(|t| SERVERS[s].bridged_for(t.client))
    }

    /// Why server `s` can't be installed while Sawit isn't signed in to GitHub, as its Install
    /// says it: GitHub's server is handed that sign-in. `None` for the others, and once signed in.
    pub(super) fn github_gate(&self, s: usize, cx: &App) -> Option<SharedString> {
        (SERVERS[s].auth == Auth::GitHub)
            .then(|| self.github.read(cx).blocked("install"))
            .flatten()
    }

    /// The scopes GitHub's server needs that the sign-in lacks (one from before Sawit asked for
    /// `workflow`): none for the other servers, or while signed out.
    pub(super) fn github_lacks(&self, s: usize, cx: &App) -> Vec<&'static str> {
        if SERVERS[s].auth != Auth::GitHub {
            return Vec::new();
        }
        self.github
            .read(cx)
            .missing_scopes()
            .into_iter()
            .filter(|scope| GITHUB_SCOPES.contains(scope))
            .collect()
    }

    /// Whether the row's Install is off: nothing installed yet, and GitHub's server while signed
    /// out. Once it's in a client, Manage still opens, so it can be removed signed out.
    pub(super) fn row_gated(&self, s: usize, cx: &App) -> bool {
        self.installs(s) == 0 && self.github_gate(s, cx).is_some()
    }

    pub(super) fn cli_for(&self, client: Client) -> Option<PathBuf> {
        client.cli().and_then(|n| self.clis.get(n).cloned())
    }

    /// Re-detects clients and reads every config once (off the UI thread and
    /// off the async workers), then asks Claude Code and Codex which servers
    /// need a sign-in.
    pub(super) fn reload(&mut self, cx: &mut Context<Self>) {
        self.loading = true;
        let server = self.server.clone();
        let rx = rt::spawn_blocking(cx, move || {
            let home = PathBuf::from(std::env::var("HOME").unwrap_or_default());
            let claude_dir = std::env::var_os("CLAUDE_CONFIG_DIR").map(PathBuf::from);
            let targets = discover(&home, claude_dir);
            Detected {
                entries: targets.iter().map(read_entries).collect(),
                targets,
                // Where the Dev Tools page finds them, not only on the app's
                // PATH: that misses what `~/.zshrc` adds (nvm, ~/.local/bin).
                programs: PROGRAMS
                    .into_iter()
                    .filter_map(|n| find_program(n).map(|p| (n, p)))
                    .collect(),
                clis: ["claude", "codex"]
                    .into_iter()
                    .filter_map(|n| find_program(n).map(|p| (n, p)))
                    .collect(),
                server_exists: server.is_file(),
            }
        });
        let pms = rt::spawn(cx, package_manager::detect());
        cx.spawn(async move |this, cx| {
            if let Ok(detected) = rx.await {
                let pms = pms.await.unwrap_or_default();
                let _ = this.update(cx, |this, cx| {
                    this.pm = package_manager::choose(
                        &pms,
                        this.pm,
                        crate::package_managers::remembered(),
                        Use::Run,
                    );
                    this.pms = pms;
                    this.apply(detected, cx)
                });
            }
        })
        .detach();
    }

    /// Takes in what a detection found.
    pub(super) fn apply(&mut self, detected: Detected, cx: &mut Context<Self>) {
        // A sheet's rows are places in the old list, which may now hold
        // other clients, or none.
        if self.sheet.is_some() && detected.targets != self.targets {
            self.sheet = None;
            self.refocus = true;
        }
        self.targets = detected.targets;
        self.entries = detected.entries;
        self.clis = detected.clis;
        self.programs = detected.programs;
        self.server_exists = detected.server_exists;
        if !self.region_chosen
            && let Some(region) = self.installed_region()
        {
            self.region = region;
        }
        self.recompute();
        self.sign_ins.clear();
        self.errors.clear();
        self.loading = false;
        let all: Vec<usize> = (0..self.targets.len()).collect();
        self.check_sign_ins(all, cx);
        cx.notify();
    }

    /// The AWS Region most clients have the server in, if any has it: the
    /// page shows that one, so an install there reads as installed, not as
    /// out of date. A tie keeps the page's.
    fn installed_region(&self) -> Option<String> {
        let mut counts: Vec<(String, usize)> = Vec::new();
        for server in SERVERS.iter().filter(|s| s.has_region()) {
            for read in &self.entries {
                if let Entries::Servers(map) = read
                    && let Some(region) = map.get(server.id).and_then(|sig| server.region_in(sig))
                {
                    match counts.iter_mut().find(|(r, _)| *r == region) {
                        Some((_, n)) => *n += 1,
                        None => counts.push((region, 1)),
                    }
                }
            }
        }
        let most = counts.iter().map(|(_, n)| *n).max()?;
        counts
            .iter()
            .find(|(r, n)| *n == most && *r == self.region)
            .or_else(|| counts.iter().find(|(_, n)| *n == most))
            .map(|(r, _)| r.clone())
    }

    /// The Region a client has a regional server in, when it's another one
    /// than the page's: installed there, not out of date.
    pub(super) fn other_region(&self, (s, t): Pair) -> Option<String> {
        let server = &SERVERS[s];
        match self.status.get(&(s, t)) {
            Some(Status::Outdated(sig)) if server.has_region() => server.region_in(sig),
            _ => None,
        }
    }

    /// Why server `s` can't be installed at all now: its program isn't
    /// there, the sheet's Region isn't one, or it is GitHub's and Sawit isn't
    /// signed in to GitHub.
    pub(super) fn install_blocked(&self, s: usize, cx: &App) -> bool {
        let region_invalid = matches!(
            &self.sheet,
            Some(Sheet::Install(sheet)) if sheet.server == s && sheet.region_invalid
        );
        (s == 0 && !self.server_exists)
            || region_invalid
            || self.github_gate(s, cx).is_some()
            || SERVERS[s]
                .missing_program_with(|c| self.programs.get(c).cloned(), self.run_pm())
                .is_some()
    }

    /// A remote server can't go into a client that reaches it through the
    /// `mcp-remote` bridge (one that only runs programs; for GitHub's, one it
    /// is handed the sign-in through the bridge) when the runner (npx, bunx,
    /// …) to run it isn't found.
    pub(super) fn bridge_blocked(&self, (s, t): Pair) -> bool {
        !self.programs.contains_key(self.run_pm().runner())
            && self
                .targets
                .get(t)
                .is_some_and(|x| SERVERS[s].bridged_for(x.client))
    }

    /// Every status, from the configs as read.
    pub(super) fn recompute(&mut self) {
        self.status.clear();
        for s in 0..SERVERS.len() {
            let spec = self.spec(s);
            for (t, read) in self.entries.iter().enumerate() {
                self.status.insert((s, t), status_in(read, &spec));
            }
        }
    }

    /// Asks each Claude Code / Codex profile in `targets` that has a remote
    /// catalog server which servers are signed in.
    pub(super) fn check_sign_ins(&mut self, targets: Vec<usize>, cx: &mut Context<Self>) {
        for t in targets {
            let Some(target) = self.targets.get(t).cloned() else {
                continue;
            };
            let Some(cli) = self.cli_for(target.client) else {
                continue;
            };
            if target.client.profile_var().is_none()
                || self.checking.contains(&t)
                || !SERVERS
                    .iter()
                    .enumerate()
                    .any(|(s, server)| server.auth == Auth::Browser && self.is_installed((s, t)))
            {
                continue;
            }
            self.checking.insert(t);
            let rx = rt::spawn(cx, async move { sign_in_states(&target, &cli).await });
            cx.spawn(async move |this, cx| {
                let result = rx.await;
                let _ = this.update(cx, |this, cx| {
                    this.checking.remove(&t);
                    if let Ok(Ok(states)) = result {
                        this.sign_ins.insert(t, states);
                    }
                    cx.notify();
                });
            })
            .detach();
        }
        cx.notify();
    }

    pub(super) fn is_installed(&self, pair: Pair) -> bool {
        matches!(
            self.status.get(&pair),
            Some(Status::Installed | Status::Outdated(_))
        )
    }

    /// Whether this profile reported that the server needs a sign-in.
    pub(super) fn needs_sign_in(&self, (s, t): Pair) -> bool {
        SERVERS[s].auth == Auth::Browser
            && self.is_installed((s, t))
            && self.sign_ins.get(&t).and_then(|m| m.get(SERVERS[s].id)) == Some(&SignIn::Needed)
    }

    pub(super) fn pending_sign_ins(&self, server: Option<usize>) -> Vec<Pair> {
        let mut out = Vec::new();
        for s in 0..SERVERS.len() {
            if server.is_some_and(|x| x != s) {
                continue;
            }
            for t in 0..self.targets.len() {
                if self.needs_sign_in((s, t)) && self.cli_for(self.targets[t].client).is_some() {
                    out.push((s, t));
                }
            }
        }
        out
    }

    /// A new region re-reads the statuses: installs elsewhere become "Other
    /// region".
    pub(super) fn set_region(&mut self, region: String, cx: &mut Context<Self>) {
        if region == self.region {
            return;
        }
        self.region = region;
        for (s, server) in SERVERS.iter().enumerate() {
            if server.has_region() {
                let spec = self.spec(s);
                // From the configs as read: no file is read per keystroke.
                for (t, read) in self.entries.iter().enumerate() {
                    self.status.insert((s, t), status_in(read, &spec));
                }
            }
        }
        cx.notify();
    }
}
