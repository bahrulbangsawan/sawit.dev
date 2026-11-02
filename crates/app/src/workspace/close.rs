//! Closing tabs: which tabs a close takes, and asking first before one goes with something it
//! would lose.

use super::pages::{skills_work, tools_work};
use super::*;

impl Workspace {
    fn flags(&self, cx: &App) -> Vec<TabFlags> {
        self.tabs
            .iter()
            .map(|t| TabFlags {
                pinned: t.pinned,
                dirty: match &t.item {
                    PaneItem::File(e) => e.read(cx).dirty,
                    // Annotation notes not yet copied or sent; a Dev Tools tab that
                    // installs something counts too: closing it stops that.
                    PaneItem::Other { view, .. } => {
                        view.clone()
                            .downcast::<BrowserView>()
                            .is_ok_and(|b| b.read(cx).unsent_notes(cx) > 0)
                            || tools_work(&t.item, cx).is_some()
                            || skills_work(&t.item, cx).is_some()
                            || new_project::new_project_work(&t.item, cx).is_some()
                    }
                    PaneItem::Server => false,
                },
            })
            .collect()
    }

    /// Closes tabs per `op` relative to `target`. Closing a single tab with
    /// unsaved changes (or a browser with notes not yet sent), or the Dev Tools
    /// tab while it installs, asks first; bulk closes leave such tabs open.
    /// Returns what the question that went up is about, if one did: the tab
    /// then closes only once the user agrees.
    pub(super) fn close(
        &mut self,
        op: CloseOp,
        target: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<&'static str> {
        let closing = to_close(op, target, &self.flags(cx));
        if let [ix] = closing[..]
            && let Some(PaneItem::File(editor)) = self.tabs.get(ix).map(|t| t.item.clone())
            && editor.read(cx).dirty
        {
            self.confirm_close_dirty(editor, window, cx);
            return Some("the file has unsaved changes");
        }
        if let [ix] = closing[..]
            && let Some(PaneItem::Other { view, .. }) = self.tabs.get(ix).map(|t| t.item.clone())
            && let Ok(browser) = view.downcast::<BrowserView>()
            && browser.read(cx).unsent_notes(cx) > 0
        {
            self.confirm_close_browser(browser, window, cx);
            return Some("the browser has annotations not yet copied or sent");
        }
        if let [ix] = closing[..]
            && let Some(work) = self.tabs.get(ix).and_then(|t| tools_work(&t.item, cx))
        {
            self.confirm_close_tools(work, window, cx);
            return Some("Dev Tools is installing, and closing the tab stops it");
        }
        if let [ix] = closing[..]
            && let Some(work) = self.tabs.get(ix).and_then(|t| skills_work(&t.item, cx))
        {
            self.confirm_close_skills(work, window, cx);
            return Some("Skills is installing, and closing the tab stops it");
        }
        if let [ix] = closing[..]
            && let Some(work) = self
                .tabs
                .get(ix)
                .and_then(|t| new_project::new_project_work(&t.item, cx))
        {
            self.confirm_close_new_project(work, window, cx);
            return Some("New Project is creating a project, which it opens when it's done");
        }
        self.remove_tabs(&closing, cx);
        None
    }

    pub(super) fn on_close_action(
        &mut self,
        op: CloseOp,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let target = self.active;
        self.close(op, target, window, cx);
    }

    /// Closes a group's tabs, like Close All: pinned and unsaved ones stay.
    pub(super) fn close_group(&mut self, root: &Path, cx: &mut Context<Self>) {
        let flags = self.flags(cx);
        let closing: Vec<usize> = (0..self.tabs.len())
            .filter(|&ix| self.tabs[ix].group.as_deref() == Some(root))
            .filter(|&ix| !flags[ix].pinned && !flags[ix].dirty)
            .collect();
        self.remove_tabs(&closing, cx);
    }

    pub(super) fn remove_tabs(&mut self, closing: &[usize], cx: &mut Context<Self>) {
        if closing.is_empty() {
            return;
        }
        let len = self.tabs.len();
        self.active = active_after(self.active, closing, len).unwrap_or(0);
        let mut ix = 0;
        self.tabs.retain(|_| {
            let keep = !closing.contains(&ix);
            ix += 1;
            keep
        });
        cx.notify();
    }

    fn confirm_close_dirty(
        &mut self,
        editor: Entity<FileEditor>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let name = editor.read(cx).title();
        let answer = window.prompt(
            PromptLevel::Warning,
            &format!("Save changes to {name}?"),
            Some("Your changes will be lost if you don't save them."),
            &["Save", "Don't Save", "Cancel"],
            cx,
        );
        cx.spawn(async move |this, cx| {
            let close = match answer.await {
                Ok(0) => editor.update(cx, |e, cx| e.save_task(cx)).await,
                Ok(1) => true,
                _ => false,
            };
            if close {
                let _ = this.update(cx, |this, cx| {
                    if let Some(ix) = this
                        .tabs
                        .iter()
                        .position(|t| matches!(&t.item, PaneItem::File(e) if *e == editor))
                    {
                        this.remove_tabs(&[ix], cx);
                    }
                });
            }
        })
        .detach();
    }

    /// Asks before closing a browser whose annotation notes weren't copied or sent: they
    /// live only in the tab.
    fn confirm_close_browser(
        &mut self,
        browser: Entity<BrowserView>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let notes = browser.read(cx).unsent_notes(cx);
        let answer = window.prompt(
            PromptLevel::Warning,
            "Close the browser with its annotations?",
            Some(&crate::browser::notes_lost(notes)),
            &["Close Anyway", "Cancel"],
            cx,
        );
        cx.spawn(async move |this, cx| {
            if let Ok(0) = answer.await {
                let _ = this.update(cx, |this, cx| {
                    if let Some(ix) = this.tabs.iter().position(|t| {
                        matches!(&t.item, PaneItem::Other { view, .. }
                            if view.clone().downcast::<BrowserView>().ok().as_ref() == Some(&browser))
                    }) {
                        this.remove_tabs(&[ix], cx);
                    }
                });
            }
        })
        .detach();
    }
}
