//! MCP page: a catalog of MCP servers (the bundled `sawit-mcp`, remote
//! servers such as AWS, Cloudflare and Neon, and local ones run with npx),
//! each installed into any AI client and profile found on this Mac. Remote
//! servers that need an account are signed in here through Claude Code's
//! and Codex's own `mcp login`, profile by profile. A server can be removed
//! from every client in one go, and an install or removal that hangs can be
//! stopped.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use futures::future::AbortHandle;
use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::input::InputState;
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use sawit_core::mcp_auth::{Login, SignIn};
use sawit_core::mcp_catalog::SERVERS;
use sawit_core::mcp_clients::{Client, Entries, Handover, Status, Target, handover};
use sawit_core::package_manager::{Available, PackageManager};

use crate::catalog;
use crate::git::account::GitHubAccount;

mod header;
mod install;
mod install_sheet;
mod list;
mod logos;
mod row;
mod sign_in;
mod sign_in_sheet;
mod state;
#[cfg(test)]
mod tests;

pub(crate) use logos::client_logo;

/// A server in the catalog and a target, by index.
type Pair = (usize, usize);

/// What a detection finds, off the UI thread.
struct Detected {
    targets: Vec<Target>,
    /// Each target's config, read once.
    entries: Vec<Entries>,
    clis: HashMap<&'static str, PathBuf>,
    programs: HashMap<&'static str, PathBuf>,
    server_exists: bool,
}

/// Programs local servers and the remote bridge run (with the package manager
/// picked), and `node`, whose folder an app without the shell's PATH is told.
const PROGRAMS: [&str; 6] = ["npx", "pnpm", "bunx", "yarn", "node", "cap"];

pub struct McpPage {
    /// `sawit-mcp` next to the running app (inside the .app bundle, or the
    /// cargo target folder during development).
    server: PathBuf,
    server_exists: bool,
    targets: Vec<Target>,
    /// Each target's config as the last detection read it, by target.
    entries: Vec<Entries>,
    status: HashMap<Pair, Status>,
    /// What Claude Code and Codex profiles report, by server name.
    sign_ins: HashMap<usize, HashMap<String, SignIn>>,
    checking: HashSet<usize>,
    clis: HashMap<&'static str, PathBuf>,
    programs: HashMap<&'static str, PathBuf>,
    region: String,
    /// The Region was typed here: a detection no longer takes it from the
    /// installed entries.
    region_chosen: bool,
    busy: HashSet<Pair>,
    /// Stops the install or removal running for a pair.
    stops: HashMap<Pair, AbortHandle>,
    errors: HashMap<Pair, String>,
    loading: bool,
    sheet: Option<Sheet>,
    focus: FocusHandle,
    filter: Filter,
    kind: Option<Kind>,
    sort: Sort,
    /// The page of the list shown, and the rows a page holds.
    paging: catalog::Paging,
    /// The search as the list last filtered by it.
    last_query: String,
    /// The row ↑ and ↓ select and ↩ opens, by its place in the list.
    cursor: Option<usize>,
    /// The page's own keyboard focus (the sheet has `focus`).
    page_focus: FocusHandle,
    /// A sheet closed: the list gets the keyboard back on the next render.
    refocus: bool,
    /// Made with the page when it has a window, else on the first render.
    search: Option<Entity<InputState>>,
    _search: Option<Subscription>,
    /// The package managers on this Mac, and the one that runs a local
    /// server's package and the remote bridge (npx, pnpm dlx, bunx, yarn dlx).
    pms: Vec<Available>,
    pm: Option<PackageManager>,
    /// The app's one GitHub sign-in, which GitHub's server is handed: installing it waits for it.
    github: Entity<GitHubAccount>,
    _github: Subscription,
}

/// Which servers the filter tabs show.
#[derive(Clone, Copy, PartialEq)]
enum Filter {
    All,
    Installed,
    Missing,
    SignIn,
}

impl Filter {
    const ALL: [Filter; 4] = [
        Filter::All,
        Filter::Installed,
        Filter::Missing,
        Filter::SignIn,
    ];

    fn label(self) -> &'static str {
        match self {
            Filter::All => "All",
            Filter::Installed => "Installed",
            Filter::Missing => "Not Installed",
            Filter::SignIn => "Needs Sign-In",
        }
    }
}

/// Where a server runs; also the catalog's order.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Kind {
    BuiltIn,
    Remote,
    Local,
}

impl Kind {
    const ALL: [Kind; 3] = [Kind::BuiltIn, Kind::Remote, Kind::Local];

    fn of(s: usize) -> Kind {
        if SERVERS[s].is_remote() {
            Kind::Remote
        } else if s == 0 {
            Kind::BuiltIn
        } else {
            Kind::Local
        }
    }

    fn label(self) -> &'static str {
        match self {
            Kind::BuiltIn => "Built In",
            Kind::Remote => "Remote",
            Kind::Local => "Runs on This Mac",
        }
    }

    fn icon(self) -> Lucide {
        match self {
            Kind::BuiltIn => Lucide::Package,
            Kind::Remote => Lucide::Globe,
            Kind::Local => Lucide::SquareTerminal,
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Sort {
    Catalog,
    Name,
    NameDesc,
    Installed,
}

impl Sort {
    const ALL: [Sort; 4] = [Sort::Catalog, Sort::Name, Sort::NameDesc, Sort::Installed];

    fn label(self) -> &'static str {
        match self {
            Sort::Catalog => "Type",
            Sort::Name => "Name (A–Z)",
            Sort::NameDesc => "Name (Z–A)",
            Sort::Installed => "Most Installed",
        }
    }
}

enum Sheet {
    Install(InstallSheet),
    SignIn(SignInFlow),
}

struct InstallSheet {
    server: usize,
    selected: HashSet<usize>,
    region: Option<Entity<InputState>>,
    /// What's typed as the Region isn't one.
    region_invalid: bool,
    _subscription: Option<Subscription>,
}

#[derive(Clone, PartialEq)]
enum Phase {
    Starting,
    Waiting,
    /// The address was typed into the client's prompt.
    Finishing,
    Failed(String),
}

struct SignInFlow {
    queue: Vec<Pair>,
    pos: usize,
    done: Vec<Pair>,
    link: Option<String>,
    address: Entity<InputState>,
    phase: Phase,
    /// The client's answer to a pasted address, or why it wasn't sent.
    problem: Option<String>,
    copied: bool,
    login: Option<Login>,
    _task: Option<Task<()>>,
    _subscription: Subscription,
}

/// "Claude Code · work", or the client alone for its only profile.
fn target_label(t: &Target) -> String {
    if t.profile == "default" {
        t.client.name().to_string()
    } else {
        format!("{} · {}", t.client.name(), t.profile)
    }
}

/// How `client` gets Sawit's GitHub sign-in for GitHub's server, as its row in the sheet says.
fn handover_note(client: Client) -> &'static str {
    match handover(client) {
        Handover::Helper => "Asks `sawit github token` for Sawit's sign-in each time it connects",
        Handover::Bridge => {
            "Gets Sawit's sign-in through the mcp-remote bridge, started by `sawit github token`"
        }
        Handover::ClientSignIn => "Signs in to GitHub itself, with its own GitHub account",
    }
}

fn plural(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

impl Render for McpPage {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.search.is_none() {
            self.make_search(window, cx);
        }
        if std::mem::take(&mut self.refocus) {
            window.focus(&self.page_focus, cx);
        }
        let query = self.query(cx);
        let overlay = match &self.sheet {
            Some(Sheet::Install(sheet)) => Some(self.render_install_sheet(sheet, cx)),
            Some(Sheet::SignIn(flow)) => Some(self.render_sign_in_sheet(flow, cx)),
            None => None,
        };
        div()
            .relative()
            .size_full()
            .key_context("McpPage")
            .track_focus(&self.page_focus)
            .on_action(cx.listener(|this, _: &catalog::FocusSearch, window, cx| {
                this.focus_search(window, cx)
            }))
            .on_action(cx.listener(|this, _: &catalog::SelectNext, window, cx| {
                this.select(true, window, cx)
            }))
            .on_action(
                cx.listener(|this, _: &catalog::SelectPrevious, window, cx| {
                    this.select(false, window, cx)
                }),
            )
            .on_action(
                cx.listener(|this, _: &catalog::Confirm, window, cx| this.confirm(window, cx)),
            )
            .bg(cx.theme().background)
            .child(
                div().id("mcp-page").size_full().overflow_y_scroll().child(
                    v_flex()
                        .p_6()
                        .gap_4()
                        .child(self.render_header(cx))
                        .child(self.render_toolbar(&query, cx))
                        .child(self.render_list(&query, cx)),
                ),
            )
            .when_some(overlay, |el, sheet| {
                el.child(
                    div()
                        .id("mcp-sheet")
                        .key_context("McpSheet")
                        .track_focus(&self.focus)
                        .absolute()
                        .inset_0()
                        .p_6()
                        .flex()
                        .items_center()
                        .justify_center()
                        .bg(gpui_kit::black().opacity(0.45))
                        .occlude()
                        .on_key_down(cx.listener(|this, e: &KeyDownEvent, _, cx| {
                            if e.keystroke.key == "escape" {
                                this.close_sheet(cx);
                                cx.stop_propagation();
                            }
                        }))
                        .child(sheet),
                )
            })
    }
}
