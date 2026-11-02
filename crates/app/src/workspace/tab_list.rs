//! The center tabs as a list: adding one to its project's group, showing, moving, pinning, folding and
//! cycling them, and finding one by its key.

use super::*;

impl Workspace {
    /// Opens an arbitrary view as a center tab (used by the database tools).
    pub fn open_item(
        &mut self,
        view: AnyView,
        key: SharedString,
        title: SharedString,
        icon: Lucide,
        cx: &mut Context<Self>,
    ) {
        self.open_item_in(view, key, title, icon, None, cx);
    }

    /// Like [`Self::open_item`], in the group of the project at `group`.
    pub(super) fn open_item_in(
        &mut self,
        view: AnyView,
        key: SharedString,
        title: SharedString,
        icon: Lucide,
        group: Option<PathBuf>,
        cx: &mut Context<Self>,
    ) {
        let item = PaneItem::Other {
            view,
            key,
            title,
            icon,
            file: None,
        };
        self.push_tab(item, group, cx);
        cx.notify();
    }

    pub(super) fn slots(&self) -> Vec<Slot<'_>> {
        self.tabs
            .iter()
            .map(|t| Slot {
                pinned: t.pinned,
                group: t.group.as_deref(),
            })
            .collect()
    }

    /// Adds a tab to the end of its project's group and shows it.
    pub(super) fn push_tab(
        &mut self,
        item: PaneItem,
        group: Option<PathBuf>,
        cx: &mut Context<Self>,
    ) {
        let at = insert_index(&self.slots(), false, group.as_deref());
        self.tabs.insert(
            at,
            Tab {
                item,
                pinned: false,
                group,
            },
        );
        self.activate_tab(at, cx);
    }

    /// Adds a tab to the end of its project's group, behind the tab on show, which stays on show:
    /// a canvas an agent's op opened.
    pub(super) fn push_tab_behind(
        &mut self,
        item: PaneItem,
        group: Option<PathBuf>,
        cx: &mut Context<Self>,
    ) {
        let had_tabs = !self.tabs.is_empty();
        let at = insert_index(&self.slots(), false, group.as_deref());
        self.tabs.insert(
            at,
            Tab {
                item,
                pinned: false,
                group,
            },
        );
        self.active = active_after_insert(self.active, at, had_tabs);
        cx.notify();
    }

    /// Shows the tab at `ix` as the user's click on it or ⌃Tab does ([`Self::activate_tab`]): a
    /// canvas's takes the keyboard too, as the canvas the user went to ([`BoardView::take_keyboard`],
    /// CANVAS-UI-2).
    pub(super) fn show_tab(&mut self, ix: usize, cx: &mut Context<Self>) {
        self.activate_tab(ix, cx);
        if let Some(board) = self.board_at(ix) {
            board.update(cx, |board, cx| board.take_keyboard(cx));
        }
    }

    /// Shows the tab at `ix`, unfolding its group and switching to its
    /// project.
    pub(super) fn activate_tab(&mut self, ix: usize, cx: &mut Context<Self>) {
        self.active = ix;
        if let Some(root) = self.tabs.get(ix).and_then(|t| t.group.clone()) {
            self.collapsed.remove(&root);
            if let Some(p) = self
                .projects
                .iter()
                .position(|p| p.root(cx) == Some(root.clone()))
            {
                self.activate_project(p, cx);
            }
        }
        cx.notify();
    }

    /// Moves the tab at `ix` into `group` (or out of any), keeping groups
    /// together.
    pub(super) fn move_to_group(
        &mut self,
        ix: usize,
        group: Option<PathBuf>,
        cx: &mut Context<Self>,
    ) {
        if ix >= self.tabs.len() || self.tabs[ix].group == group {
            return;
        }
        let mut tab = self.tabs.remove(ix);
        tab.group = group;
        let at = insert_index(&self.slots(), tab.pinned, tab.group.as_deref());
        self.tabs.insert(at, tab);
        self.active = active_after_move(self.active, ix, at);
        cx.notify();
    }

    pub(super) fn toggle_group(&mut self, root: PathBuf, cx: &mut Context<Self>) {
        if !self.collapsed.remove(&root) {
            self.collapsed.insert(root);
        }
        cx.notify();
    }

    /// Activates the Server tab, reopening it (after pinned tabs) if closed.
    pub(super) fn show_server_tab(&mut self) {
        match self
            .tabs
            .iter()
            .position(|t| matches!(t.item, PaneItem::Server))
        {
            Some(ix) => self.active = ix,
            None => {
                let at = self.tabs.iter().take_while(|t| t.pinned).count();
                self.tabs.insert(
                    at,
                    Tab {
                        item: PaneItem::Server,
                        pinned: false,
                        group: None,
                    },
                );
                self.active = at;
            }
        }
    }

    pub(super) fn toggle_pin(&mut self, ix: usize, cx: &mut Context<Self>) {
        let Some(tab) = self.tabs.get(ix).cloned() else {
            return;
        };
        self.tabs.remove(ix);
        // Pinned tabs sit together at the start, like in Zed; a pinned tab
        // goes to the end of them, an unpinned one back into its group.
        let pinned = !tab.pinned;
        let at = insert_index(&self.slots(), pinned, tab.group.as_deref());
        self.tabs.insert(at, Tab { pinned, ..tab });
        self.active = active_after_move(self.active, ix, at);
        cx.notify();
    }

    pub(super) fn toggle_read_only(&mut self, ix: usize, cx: &mut Context<Self>) {
        if let Some(PaneItem::File(editor)) = self.tabs.get(ix).map(|t| t.item.clone()) {
            editor.update(cx, |e, cx| {
                let ro = !e.read_only;
                e.set_read_only(ro, cx)
            });
        }
    }

    /// Shows the tab `step` places after (or before) the active one.
    pub(super) fn cycle_tab(&mut self, step: isize, cx: &mut Context<Self>) {
        let len = self.tabs.len() as isize;
        if len > 1 {
            let ix = (self.active as isize + step).rem_euclid(len) as usize;
            self.show_center(cx);
            self.show_tab(ix, cx);
        }
    }

    /// The view the tab at `ix` shows: what tells it apart from a tab that takes its place. The
    /// tab bar's scroll and the keyboard's give-back ([`Shown`]) both go by it.
    pub(super) fn tab_view(&self, ix: usize) -> Option<EntityId> {
        Some(match &self.tabs.get(ix)?.item {
            PaneItem::Server => self.server_detail.entity_id(),
            PaneItem::File(editor) => editor.entity_id(),
            PaneItem::Other { view, .. } => view.entity_id(),
        })
    }

    /// `key` for the active project: tabs like the commit graph are one
    /// per project, not one per window.
    pub(super) fn project_key(&self, key: &str, cx: &App) -> String {
        match self.root(cx) {
            Some(root) => format!("{key}\u{0}{}", root.display()),
            None => key.to_string(),
        }
    }

    /// Activates the tab with `key`, if one is open.
    pub(super) fn activate_key(&mut self, key: &str) -> bool {
        match self
            .tabs
            .iter()
            .position(|t| matches!(&t.item, PaneItem::Other { key: k, .. } if k.as_ref() == key))
        {
            Some(ix) => {
                self.active = ix;
                true
            }
            None => false,
        }
    }
}
