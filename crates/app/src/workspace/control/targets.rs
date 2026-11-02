//! What an agent's command acts on: its project's browser or canvas, the folder its shell starts
//! in, and the page, tab, SSH server, database connection or canvas file it names.

use super::*;

/// Where an agent's browser op goes.
#[derive(Debug, PartialEq)]
pub(super) enum BrowserRoute {
    /// To the project's browser as it is.
    Use,
    /// To the project's browser, brought forward (or opened).
    Show,
    /// Nowhere: the project has no browser.
    Closed,
}

/// Only opening a page, a screenshot or a device size brings the browser forward (`shows_tab`);
/// other ops work with it in the background, and with none open they say so rather than open
/// one over what the user is looking at.
pub(super) fn browser_route(open: bool, shown: bool, shows_tab: bool) -> BrowserRoute {
    match (open, shows_tab) {
        (true, false) => BrowserRoute::Use,
        (true, true) if shown => BrowserRoute::Use,
        (_, true) => BrowserRoute::Show,
        (false, false) => BrowserRoute::Closed,
    }
}

/// The project whose browser or canvas an agent's op goes to (`None`: the browser or the canvas
/// for no project), from the window's project folders, `roots`. With no project named, the one
/// holding the folder the agent works in, `agent_folder`: its browser_navigate opens that
/// project's dev server, the page's cookies, screenshots and exports are that project's
/// (final-seams m1), and its canvas is that project's `.nyawit/canvas.sawit`, the one
/// sawit-mcp edits while the app is closed (Q12). Else the active project's, which is the one it
/// named, if it named one ([`crate::cli`] makes it active); a canvas op's named project comes as
/// its `agent_folder` instead, so that the user's active project stays.
pub(super) fn agent_project(
    roots: &[PathBuf],
    agent_folder: Option<&Path>,
    active: Option<PathBuf>,
) -> Option<PathBuf> {
    agent_folder
        .and_then(|folder| crate::tabs::project_of(roots, folder))
        .or(active)
}

/// For a canvas `mermaid` op, the work that renders its diagram and makes it into skeletons
/// (`sawit_board::ops::prepare_diagram`), to run off the UI thread; `None` for any other op.
pub(super) fn mermaid_op(
    op: &Value,
) -> Option<impl Future<Output = sawit_board::ops::PreparedDiagram> + use<>> {
    (op.get("op").and_then(Value::as_str) == Some("mermaid")).then(|| {
        let op = op.clone();
        async move { sawit_board::ops::prepare_diagram(&op).expect("a mermaid op") }
    })
}

/// The kind of canvas file `file` is, which an agent's canvas op names: an absolute path to a
/// `.sawit` or an `.excalidraw` file. Anything else is the op's error, and no tab opens for it.
pub(super) fn canvas_file_kind(file: &Path) -> Result<FileKind, String> {
    if !file.is_absolute() {
        return Err(format!(
            "{} isn't an absolute path: name the canvas file as one.",
            file.display()
        ));
    }
    match classify(file) {
        kind @ (FileKind::Canvas | FileKind::Excalidraw) => Ok(kind),
        _ => Err(format!(
            "{} isn't a canvas file: name a .sawit or .excalidraw file.",
            file.display()
        )),
    }
}

/// Where a new shell an agent starts goes: `cwd` when it names a folder, else the agent's own
/// project, as for its browser and canvas ([`agent_project`]): the project of the window's `roots`
/// that holds the folder the agent works in, else the active one. It started in the active
/// project, so an agent in project A whose window showed B ran its command in B's folder.
pub(super) fn agent_shell_folder(
    cwd: Option<PathBuf>,
    roots: &[PathBuf],
    agent_folder: Option<&Path>,
    active: Option<PathBuf>,
) -> Option<PathBuf> {
    cwd.or_else(|| agent_project(roots, agent_folder, active))
}

/// The folder a shell an agent starts goes to, `cwd`, if it is one: an absolute path to a folder
/// that is there. The PTY ignores a `chdir` that fails, so anything else would start the shell in
/// the app's own folder (`/` for Sawit.app), and the agent's command would run there (MCP-4).
pub(super) fn shell_folder(cwd: Option<PathBuf>) -> Result<Option<PathBuf>, String> {
    match cwd {
        Some(dir) if !dir.is_absolute() => Err(format!(
            "{} isn't an absolute path: give the folder to run in as one.",
            dir.display()
        )),
        Some(dir) if !dir.is_dir() => Err(format!(
            "{} isn't a folder: nothing was run.",
            dir.display()
        )),
        cwd => Ok(cwd),
    }
}

/// The terminal's way of splitting a pane, for the way `sawit-mcp` names.
pub(super) fn split_dir(
    direction: sawit_core::app_control::SplitDirection,
) -> sawit_core::terminal::SplitDir {
    use sawit_core::app_control::SplitDirection;
    use sawit_core::terminal::SplitDir;
    match direction {
        SplitDirection::Right => SplitDir::Right,
        SplitDirection::Down => SplitDir::Down,
    }
}

pub(super) fn sidebar_tab(panel: Panel) -> SidebarTab {
    match panel {
        Panel::Instances => SidebarTab::Servers,
        Panel::Files => SidebarTab::Files,
        Panel::Git => SidebarTab::Git,
        Panel::Database => SidebarTab::Database,
        Panel::Run => SidebarTab::Run,
        Panel::Ports => SidebarTab::Ports,
        Panel::Network => SidebarTab::Network,
        Panel::Prompts => SidebarTab::Prompts,
        Panel::Ssh => SidebarTab::Ssh,
        Panel::ShellConfig => SidebarTab::ShellConfig,
    }
}

impl Workspace {
    pub(super) fn open_page(
        &mut self,
        page: Page,
        agent_folder: Option<&Path>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        match page {
            // The agent's project's, as for its canvas and browser ops.
            Page::Canvas => {
                let root = agent_project(&self.roots(cx), agent_folder, self.root(cx));
                self.open_canvas_of(root, cx);
            }
            // The agent's project's, as for its browser ops.
            Page::Browser => {
                let root = agent_project(&self.roots(cx), agent_folder, self.root(cx));
                self.open_browser_of(root, None, window, cx);
            }
            // An agent's: the keyboard stays where the user has it.
            Page::Mcp => self.show_mcp_page(false, window, cx),
            Page::Tools => self.show_tools_page(false, window, cx),
            Page::Skills => self.show_skills_page(false, window, cx),
            Page::Changelog => {
                self.show_center(cx);
                self.open_changelog_page(cx)
            }
            Page::GitGraph | Page::Github => {
                if self.root(cx).is_none() {
                    return Err("Open a project folder first.".into());
                }
                self.show_center(cx);
                match page {
                    Page::GitGraph => self.open_git_graph(cx),
                    _ => self.open_github(window, cx),
                }
            }
            Page::Instance => {
                self.show_center(cx);
                self.show_server_tab();
            }
            // An agent's too: a ⌫ meant for the terminal would remove the selected binding.
            Page::Keymap => self.show_keymap_editor(false, window, cx),
            Page::NewProject => {
                self.show_new_project_page(false, window, cx);
            }
        }
        Ok(())
    }

    /// The canvas of the canvas file an agent's op names, `file`: its tab, opened as `sawit <file>`
    /// opens it ([`Self::open_board_file`]: a project's canvas file is that project's canvas tab,
    /// and an `.excalidraw` file's canvas is bound to the `.sawit` file beside it), shown with
    /// the tab area and its project, but with the keyboard left where the user has it.
    pub(super) fn agent_canvas_file(
        &mut self,
        file: PathBuf,
        cx: &mut Context<Self>,
    ) -> Result<Entity<BoardView>, String> {
        let kind = canvas_file_kind(&file)?;
        let board = self.open_board_file(file, kind, super::super::canvas::Reach::Quietly, cx);
        board.ok_or_else(|| "The canvas didn't open.".into())
    }

    /// A center tab by index (from `state`) or title: exact (any case),
    /// else the only title containing it.
    pub(super) fn find_tab(&self, tab: &str, cx: &App) -> Result<usize, String> {
        let titles: Vec<String> = self
            .tabs
            .iter()
            .map(|t| match &t.item {
                PaneItem::Server => "Instance".to_string(),
                PaneItem::File(e) => e.read(cx).title(),
                PaneItem::Other { title, .. } => title.to_string(),
            })
            .collect();
        if let Ok(ix) = tab.trim().parse::<usize>() {
            return match ix < titles.len() {
                true => Ok(ix),
                false => Err(format!("There is no tab {ix}; {} are open.", titles.len())),
            };
        }
        let wanted = tab.to_lowercase();
        if let Some(ix) = titles.iter().position(|t| t.to_lowercase() == wanted) {
            return Ok(ix);
        }
        let partial: Vec<usize> = (0..titles.len())
            .filter(|&ix| titles[ix].to_lowercase().contains(&wanted))
            .collect();
        match partial.as_slice() {
            [ix] => Ok(*ix),
            [] => Err(format!(
                "No tab is called “{tab}”. Open tabs: {}.",
                titles.join(", ")
            )),
            _ => Err(format!(
                "Several tabs match “{tab}”: {}. Use the index.",
                partial
                    .iter()
                    .map(|&ix| format!("{ix} {}", titles[ix]))
                    .collect::<Vec<_>>()
                    .join(", ")
            )),
        }
    }

    /// The store key of an SSH server: a saved one by id or name, else a
    /// `~/.ssh/config` alias.
    pub(super) fn ssh_key(&self, host: &str, cx: &App) -> Result<String, String> {
        let store = self.ssh_store.read(cx);
        if let Some(h) = store.list.get(host) {
            return Ok(crate::ssh::saved_key(&h.id));
        }
        let named: Vec<_> = store
            .list
            .hosts
            .iter()
            .filter(|h| h.name == host || h.label() == host)
            .collect();
        match named.as_slice() {
            [h] => return Ok(crate::ssh::saved_key(&h.id)),
            [] => {}
            _ => {
                return Err(format!(
                    "Several saved servers are called “{host}”. Use the id from list_ssh_hosts."
                ));
            }
        }
        if store.config.iter().any(|c| c.alias == host) {
            return Ok(crate::ssh::alias_key(host));
        }
        Err(format!(
            "“{host}” isn't a saved server or a ~/.ssh/config host. See list_ssh_hosts."
        ))
    }

    /// The Database panel's connection for `name`: a saved connection, else
    /// a Sawit instance (added as “name (Sawit)”, like Connect Instances).
    pub(super) fn db_profile(
        &mut self,
        name: &str,
        cx: &mut Context<Self>,
    ) -> Result<String, String> {
        let store = self.db.read(cx);
        if store.profile(name).is_some() {
            return Ok(name.to_string());
        }
        // A D1 or SQLite id from detect_databases: the store opens it when first asked.
        if let Some(found) = sawit_core::db::d1::resolve(name) {
            return found.map(|_| name.to_string());
        }
        let as_instance = format!("{name} (Sawit)");
        if store.profile(&as_instance).is_some() {
            return Ok(as_instance);
        }
        let port = self
            .servers
            .read(cx)
            .registry
            .get(name)
            .map(|i| i.port)
            .ok_or_else(|| {
                format!(
                    "“{name}” isn't a saved connection or a Sawit instance. See list_connections."
                )
            })?;
        let profile = ConnProfile {
            name: as_instance.clone(),
            ..ConnProfile::new("localhost", port, "postgres", "postgres")
        };
        self.db.update(cx, |s, cx| s.add_profile(profile, cx));
        Ok(as_instance)
    }

    /// Opens a table, SQL or diagram tab; it appears once connected.
    pub(super) fn open_db(
        &mut self,
        event: DbEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<Answer, String> {
        self.show_center(cx);
        self.layout.sidebar = true;
        self.sidebar = SidebarTab::Database;
        self.on_db_event(&event, window, cx);
        let waiting = self.pending_db.is_some();
        Ok(Answer::Now(json!({
            "tab": (!waiting).then_some(self.active),
            "connecting": waiting,
        })))
    }
}
