//! An agent's browser ops on a tab of its choosing, and its `browser_tabs` and `canvas_tabs`.
//!
//! An op on a tab not on show runs there, the tab woken off screen if it gave its web view up
//! ([`BrowserTabs::wake`]): the user's tab on show, the browser's place and the keyboard stay as
//! they are. Without a tab the op goes to the tab on show, as before tabs, and the ops that show a
//! page bring the browser forward ([`browser_route`]).

use super::targets::{BrowserRoute, browser_route};
use super::*;

impl Workspace {
    /// Runs `op` for `agent` on the browser of `root`, on its tab named `tab` if one is.
    pub(super) fn agent_browser_op(
        &mut self,
        root: Option<PathBuf>,
        op: sawit_core::browser::BrowserOp,
        tab: Option<String>,
        agent: Option<&AgentIdentity>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<Answer, String> {
        if let Some(name) = tab {
            let (_, tabs) = self
                .browser_tabs_of(root.as_deref())
                .ok_or(sawit_core::browser::NOT_OPEN)?;
            let (ix, view) = tabs.read(cx).find(&name, cx).ok_or_else(|| no_tab(&name))?;
            if !view.read(cx).on_show() {
                tabs.update(cx, |t, cx| t.wake(ix, window, cx));
            }
            return Ok(Answer::Later(
                view.update(cx, |b, cx| b.apply_as(op, agent, cx)),
            ));
        }
        let open = self.browser_tabs_of(root.as_deref()).map(|(_, t)| t);
        let shown = open.is_some() && open == self.active_browser_tabs();
        let browser = match (browser_route(open.is_some(), shown, op.shows_tab()), open) {
            (BrowserRoute::Use, Some(tabs)) => {
                let (ix, view) = {
                    let t = tabs.read(cx);
                    (t.counts().1, t.active_view())
                };
                // A browser in the background may have given its page up.
                if !view.read(cx).on_show() {
                    tabs.update(cx, |t, cx| t.wake(ix, window, cx));
                }
                view
            }
            (BrowserRoute::Closed, _) => return Err(sawit_core::browser::NOT_OPEN.into()),
            _ => self.open_browser_of(root, None, window, cx),
        };
        Ok(Answer::Later(
            browser.update(cx, |b, cx| b.apply_as(op, agent, cx)),
        ))
    }

    /// `browser_tabs`: lists the tabs of the browser of `root`, opens one (at `url`, behind the
    /// one on show unless `show`), closes one or shows one.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn agent_browser_tabs(
        &mut self,
        root: Option<PathBuf>,
        action: &str,
        tab: Option<String>,
        url: Option<String>,
        show: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<Value, String> {
        let open = self.browser_tabs_of(root.as_deref()).map(|(_, t)| t);
        let tabs = match (action, open) {
            (_, Some(tabs)) => tabs,
            ("open", None) => {
                // The browser opens with its first tab: at `url`, behind the tab on show of the
                // window unless `show`.
                let active = self.active;
                let view = self.open_browser_of(root.clone(), url.clone(), window, cx);
                if !show {
                    self.activate_tab(active, cx);
                }
                let (_, tabs) = self
                    .browser_tabs_of(root.as_deref())
                    .ok_or("The browser didn't open.")?;
                let _ = view;
                return Ok(
                    json!({ "opened": tabs.read(cx).id_of(0), "tabs": tabs.read(cx).describe(cx) }),
                );
            }
            ("list", None) => return Ok(json!({ "open": false, "tabs": [] })),
            (_, None) => return Err(sawit_core::browser::NOT_OPEN.into()),
        };
        let named = |cx: &App| -> Result<usize, String> {
            let name = tab
                .as_deref()
                .ok_or("Name the tab: its id, title or address.")?;
            tabs.read(cx)
                .find(name, cx)
                .map(|(ix, _)| ix)
                .ok_or_else(|| no_tab(name))
        };
        let answer = match action {
            "list" => json!({ "open": true }),
            "open" => {
                let url = url.as_deref().map(crate::browser::address_url);
                tabs.update(cx, |t, cx| t.open_tab(url, show, window, cx));
                let (count, _) = tabs.read(cx).counts();
                json!({ "opened": tabs.read(cx).id_of(count - 1) })
            }
            "close" => {
                let ix = named(cx)?;
                let notes = tabs.read(cx).views()[ix].read(cx).unsent_notes(cx);
                if notes > 0 {
                    return Err(format!(
                        "That tab has {notes} annotation note(s) not copied or sent; the user closes it, after being asked."
                    ));
                }
                let id = tabs.read(cx).id_of(ix);
                tabs.update(cx, |t, cx| t.close_tab(ix, window, cx));
                json!({ "closed": id })
            }
            "activate" => {
                let ix = named(cx)?;
                tabs.update(cx, |t, cx| t.activate(ix, cx));
                if show && let Some((at, _)) = self.browser_tabs_of(root.as_deref()) {
                    self.activate_tab(at, cx);
                }
                json!({ "active": tabs.read(cx).id_of(ix) })
            }
            other => {
                return Err(format!(
                    "Unknown action “{other}”: use list, open, close or activate."
                ));
            }
        };
        let mut answer = answer;
        answer["tabs"] = tabs.read(cx).describe(cx);
        Ok(answer)
    }
}

impl Workspace {
    /// `canvas_tabs`: lists the canvases of the project at `root` (open in a tab or not), opens one
    /// (by name, made new when there is none; behind the tab on show unless `show`), closes one,
    /// shows one or renames one. Deleting a canvas is the user's, after being asked.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn agent_canvas_tabs(
        &mut self,
        root: Option<PathBuf>,
        action: &str,
        tab: Option<String>,
        name: Option<String>,
        show: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<Value, String> {
        let _ = window;
        let root = root.ok_or("Canvas tabs are a project's: open a project folder first.")?;
        let named = || -> Result<PathBuf, String> {
            let tab = tab
                .as_deref()
                .ok_or("Name the canvas: canvas, or another's name.")?;
            sawit_core::canvases::canvas_file(&root, tab)
        };
        let reach = if show {
            super::super::canvas::Reach::Show
        } else {
            super::super::canvas::Reach::Quietly
        };
        let answer = match action {
            "list" => json!({}),
            "open" | "activate" => {
                let file = named()?;
                if action == "activate" && !file.is_file() && self.board_tab(&file, cx).is_none() {
                    return Err(format!(
                        "No canvas is “{}”. canvas_tabs list shows them.",
                        tab.unwrap_or_default()
                    ));
                }
                self.open_canvas_file(file.clone(), reach, cx)
                    .ok_or("The canvas didn't open.")?;
                if action == "activate"
                    && let Some((ix, _)) = self.board_tab(&file, cx)
                {
                    self.show_tab(ix, cx);
                }
                json!({ "canvas": sawit_core::canvases::canvas_name(&file) })
            }
            "close" => {
                let file = named()?;
                let (ix, _) = self
                    .board_tab(&file, cx)
                    .ok_or("That canvas isn't open in a tab.")?;
                self.remove_tabs(&[ix], cx);
                json!({ "closed": sawit_core::canvases::canvas_name(&file) })
            }
            "rename" => {
                let file = named()?;
                let to = name.as_deref().ok_or("Give the canvas's new name.")?;
                let to = self.rename_canvas(&file, to, cx)?;
                json!({ "renamed": sawit_core::canvases::canvas_name(&to) })
            }
            other => {
                return Err(format!(
                    "Unknown action “{other}”: use list, open, close, activate or rename."
                ));
            }
        };
        let mut answer = answer;
        answer["canvases"] = self.describe_canvases(&root, cx);
        Ok(answer)
    }

    /// The canvases of `root` as `canvas_tabs` and `app_state` list them.
    pub(super) fn describe_canvases(&self, root: &Path, cx: &App) -> Value {
        let open = self.open_canvases(root, cx);
        let active = self
            .board_at(self.active)
            .map(|b| b.read(cx).path().to_path_buf());
        let mut files = sawit_core::canvases::canvases(root);
        for file in &open {
            if !files.contains(file) {
                files.push(file.clone());
            }
        }
        json!(
            files
                .iter()
                .map(|f| json!({
                    "name": sawit_core::canvases::canvas_name(f),
                    "file": f,
                    "open": open.contains(f),
                    "active": active.as_ref() == Some(f),
                }))
                .collect::<Vec<_>>()
        )
    }
}

fn no_tab(name: &str) -> String {
    format!("No browser tab is “{name}”. browser_tabs lists them by id, title and address.")
}
