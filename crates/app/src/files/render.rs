//! Drawing the tree: its rows, flattened for the virtual list, the name box, header and menu.

use std::path::Path;
use std::rc::Rc;

use gpui_kit::component::button::*;
use gpui_kit::component::input::{Escape, Input};
use gpui_kit::component::menu::{ContextMenuExt, PopupMenu};
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use sawit_core::files::Entry;

use super::{
    AddToGitExclude, AddToGitignore, CollapseAll, CopyEntry, CopyPath, CopyRelativePath, Cut,
    Delete, DraggedFile, Duplicate, Edit, EditKind, ExpandAll, FilesEvent, FilesPanel,
    FindInFolder, NewFile, NewFolder, OpenInDefaultApp, OpenInTerminal, Paste, Redo, Rename,
    Request, RevealInFinder, Row, RowsOf, Trash, TrashWithoutAsking, Undo, ViewHistory,
};
use crate::states::{Banner, Placeholder};

const ROW_HEIGHT: f32 = 22.;

impl FilesPanel {
    /// Flattens the tree's rows again if what they come from changed since: a render for a
    /// hover, a scroll or the keyboard leaves them as they are.
    fn update_rows(&mut self) {
        let of = RowsOf {
            children: self.children.version(),
            expanded: self.expanded.version(),
            root: self.root.clone(),
            edit: (self.edit.as_ref()).map(|edit| (edit.kind.clone(), edit.parent.clone())),
        };
        if self.rows_of.as_ref() != Some(&of) {
            self.rows = Rc::new(self.flat());
            self.rows_of = Some(of);
        }
    }

    fn rows(&self, dir: &Path, depth: usize, out: &mut Vec<Row>) {
        if let Some(edit) = &self.edit
            && matches!(edit.kind, EditKind::New { .. })
            && edit.parent == dir
        {
            out.push(Row::Edit(depth));
        }
        let Some(entries) = self.children.get(dir) else {
            return;
        };
        for entry in entries {
            let renaming = self
                .edit
                .as_ref()
                .is_some_and(|e| matches!(&e.kind, EditKind::Rename(p) if *p == entry.path));
            out.push(if renaming {
                Row::Edit(depth)
            } else {
                Row::Entry(entry.clone(), depth)
            });
            if entry.is_dir && self.expanded.contains(&entry.path) {
                self.rows(&entry.path, depth + 1, out);
            }
        }
    }

    fn flat(&self) -> Vec<Row> {
        let mut flat = Vec::new();
        if let Some(root) = &self.root {
            self.rows(root, 0, &mut flat);
        }
        flat
    }

    /// The row of `entry`. The selected one is outlined while the tree has the keyboard (`keys`),
    /// as in Zed, so that the user can tell whether keys go to it.
    fn render_row(
        &self,
        ix: usize,
        entry: Entry,
        depth: usize,
        keys: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let selected = self.selected.as_ref() == Some(&entry.path);
        let muted = cx.theme().muted_foreground;
        let open = entry.is_dir && self.expanded.contains(&entry.path);
        let (chevron, icon) = if entry.is_dir {
            (
                Some(if open {
                    IconName::ChevronDown
                } else {
                    IconName::ChevronRight
                }),
                Icon::new(if open {
                    IconName::FolderOpen
                } else {
                    IconName::Folder
                })
                .xsmall()
                .text_color(muted)
                .into_any_element(),
            )
        } else {
            (None, crate::file_icons::file_icon(&entry.path, cx))
        };
        let name = entry.name.clone();
        let cut = self
            .clip
            .as_ref()
            .is_some_and(|c| c.cut && c.path == entry.path);
        let path = entry.path.clone();
        let menu = self.menu_state();
        let focus = self.focus.clone();
        let dragged = DraggedFile {
            path: entry.path.clone(),
            name: entry.name.clone().into(),
        };
        h_flex()
            .id(("file-row", ix))
            .role(Role::TreeItem)
            .aria_label(name.clone())
            .aria_level(depth + 1)
            .aria_selected(selected)
            .when(entry.is_dir, |el| el.aria_expanded(open))
            .w_full()
            .h(px(ROW_HEIGHT))
            .pl(px(4. + depth as f32 * 12.))
            .pr_2()
            .gap_1()
            .text_sm()
            .cursor_pointer()
            .border_1()
            .border_color(transparent_black())
            .when(selected, |el| el.bg(cx.theme().list_active))
            .when(selected && keys, |el| {
                el.border_color(cx.theme().list_active_border)
            })
            .when(!selected, |el| el.hover(|el| el.bg(cx.theme().list_hover)))
            .when(cut, |el| el.opacity(0.5))
            .child(
                div()
                    .w(px(14.))
                    .flex_none()
                    .children(chevron.map(|c| Icon::new(c).xsmall().text_color(muted))),
            )
            .child(icon)
            .child(div().truncate().child(name))
            .on_drag(dragged, |dragged, _, _, cx| cx.new(|_| dragged.clone()))
            .on_click(cx.listener(move |this, _, window, cx| this.toggle(&entry, window, cx)))
            // The menu's commands act on the row clicked.
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |this, _, _, cx| {
                    this.selected = Some(path.clone());
                    cx.notify();
                }),
            )
            .context_menu(move |m, _, _| file_menu(m, focus.clone(), menu))
            .into_any_element()
    }

    fn render_edit(&self, ix: usize, depth: usize, cx: &mut Context<Self>) -> AnyElement {
        let muted = cx.theme().muted_foreground;
        let icon = match &self.edit {
            Some(Edit {
                kind: EditKind::New { dir: true },
                ..
            }) => Icon::new(IconName::Folder)
                .xsmall()
                .text_color(muted)
                .into_any_element(),
            Some(Edit {
                kind: EditKind::Rename(path),
                ..
            }) if path.is_dir() => Icon::new(IconName::Folder)
                .xsmall()
                .text_color(muted)
                .into_any_element(),
            Some(Edit {
                kind: EditKind::Rename(path),
                ..
            }) => crate::file_icons::file_icon(path, cx),
            _ => Icon::new(IconName::File)
                .xsmall()
                .text_color(muted)
                .into_any_element(),
        };
        h_flex()
            .id(("file-row", ix))
            .w_full()
            .h(px(ROW_HEIGHT))
            .pl(px(4. + depth as f32 * 12.))
            .pr_2()
            .gap_1()
            // In line with the rows' outline.
            .border_1()
            .border_color(transparent_black())
            .child(div().w(px(14.)).flex_none())
            .child(icon)
            .children(self.edit.as_ref().map(|e| {
                div()
                    .flex_1()
                    .min_w_0()
                    .on_action(
                        cx.listener(|this, _: &Escape, window, cx| this.cancel_edit(window, cx)),
                    )
                    .child(Input::new(&e.input).xsmall())
            }))
            .into_any_element()
    }

    pub(super) fn menu_state(&self) -> MenuState {
        MenuState {
            paste: self.clip.is_some(),
            undo: !self.undo.is_empty(),
            redo: !self.redo.is_empty(),
            git: self.repo.is_some(),
        }
    }
}

impl Render for FilesPanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.update_rows();
        let count = self.rows.len();
        let title = self
            .root
            .as_ref()
            .and_then(|r| r.file_name())
            .map(|n| n.to_string_lossy().to_uppercase())
            .unwrap_or_else(|| "FILES".into());

        v_flex()
            .size_full()
            .bg(cx.theme().sidebar)
            .key_context("FilesPanel")
            .track_focus(&self.focus)
            .on_action(cx.listener(|this, _: &NewFile, w, cx| this.start_new(false, w, cx)))
            .on_action(cx.listener(|this, _: &NewFolder, w, cx| this.start_new(true, w, cx)))
            .on_action(cx.listener(|this, _: &RevealInFinder, _, cx| {
                if let Some(path) = this.target() {
                    cx.reveal_path(&path);
                }
            }))
            .on_action(cx.listener(|this, _: &OpenInDefaultApp, _, cx| {
                if let Some(path) = this.target() {
                    cx.open_with_system(&path);
                }
            }))
            .on_action(cx.listener(|this, _: &OpenInTerminal, _, cx| {
                if let Some(dir) = this.target_dir() {
                    cx.emit(FilesEvent::OpenTerminal(dir));
                }
            }))
            .on_action(cx.listener(|this, _: &FindInFolder, _, cx| {
                if let Some(dir) = this.target_dir() {
                    cx.emit(FilesEvent::FindInFolder(dir));
                }
            }))
            .on_action(cx.listener(|this, _: &Cut, _, cx| this.pick(true, cx)))
            .on_action(cx.listener(|this, _: &CopyEntry, _, cx| this.pick(false, cx)))
            .on_action(cx.listener(|this, _: &Duplicate, w, cx| this.duplicate(w, cx)))
            .on_action(cx.listener(|this, _: &Paste, w, cx| this.paste(w, cx)))
            .on_action(cx.listener(|this, _: &Undo, w, cx| this.request(Request::Undo, w, cx)))
            .on_action(cx.listener(|this, _: &Redo, w, cx| this.request(Request::Redo, w, cx)))
            .on_action(cx.listener(|this, _: &CopyPath, _, cx| this.copy_path(false, cx)))
            .on_action(cx.listener(|this, _: &CopyRelativePath, _, cx| this.copy_path(true, cx)))
            .on_action(cx.listener(|this, _: &AddToGitignore, _, cx| this.ignore(false, cx)))
            .on_action(cx.listener(|this, _: &AddToGitExclude, _, cx| this.ignore(true, cx)))
            .on_action(cx.listener(|this, _: &ViewHistory, _, cx| {
                if let Some(path) = this.target() {
                    cx.emit(FilesEvent::ViewHistory(path));
                }
            }))
            .on_action(cx.listener(|this, _: &Rename, w, cx| this.start_rename(w, cx)))
            .on_action(cx.listener(|this, _: &Trash, w, cx| this.trash(true, w, cx)))
            .on_action(cx.listener(|this, _: &TrashWithoutAsking, w, cx| this.trash(false, w, cx)))
            .on_action(cx.listener(|this, _: &Delete, w, cx| this.delete(w, cx)))
            .on_action(cx.listener(|this, _: &ExpandAll, _, cx| this.expand_all(cx)))
            .on_action(cx.listener(|this, _: &CollapseAll, _, cx| this.collapse_all(cx)))
            .child(
                h_flex()
                    .h(px(32.))
                    .px_2()
                    .gap_0p5()
                    .border_b_1()
                    .border_color(cx.theme().border)
                    .child(
                        div()
                            .flex_1()
                            .text_xs()
                            .font_semibold()
                            .text_color(cx.theme().muted_foreground)
                            .truncate()
                            .child(title),
                    )
                    .when(self.busy, |el| {
                        el.child(
                            div()
                                .id("files-busy")
                                .px_1()
                                .child(Spinner::new().xsmall().color(cx.theme().muted_foreground))
                                .tooltip(|window, cx| {
                                    Tooltip::new("Changing files…").build(window, cx)
                                }),
                        )
                    })
                    .when(self.root.is_some(), |el| {
                        el.child(
                            Button::new("refresh-files")
                                .ghost()
                                .xsmall()
                                .icon(IconName::RotateCw)
                                .accessibility_label("Reload File List")
                                .tooltip("Reload File List")
                                .on_click(cx.listener(|this, _, _, cx| this.refresh(cx))),
                        )
                    })
                    .child(
                        Button::new("open-folder")
                            .ghost()
                            .xsmall()
                            .icon(IconName::FolderOpen)
                            .accessibility_label("Open Project Folder")
                            .tooltip("Open Project Folder")
                            .on_click(cx.listener(|this, _, _, cx| this.prompt_open(cx))),
                    ),
            )
            .when_some(self.error.clone(), |el, e| {
                el.child(Banner::error("files-error", e).on_dismiss(cx.listener(
                    |this, _, _, cx| {
                        this.error = None;
                        cx.notify();
                    },
                )))
            })
            .child(
                v_flex()
                    .id("file-tree")
                    .role(Role::Tree)
                    .aria_label("Files")
                    .flex_1()
                    .min_h_0()
                    .py_1()
                    .when(count > 0, |el| {
                        // Virtual: only visible rows are built.
                        el.child(
                            uniform_list(
                                "file-rows",
                                count,
                                cx.processor(|this, range: std::ops::Range<usize>, window, cx| {
                                    let rows = this.rows.clone();
                                    let keys = this.focus.contains_focused(window, cx);
                                    range
                                        .filter_map(|ix| {
                                            Some(match rows.get(ix)?.clone() {
                                                Row::Entry(entry, depth) => {
                                                    this.render_row(ix, entry, depth, keys, cx)
                                                }
                                                Row::Edit(depth) => this.render_edit(ix, depth, cx),
                                            })
                                        })
                                        .collect()
                                }),
                            )
                            .track_scroll(&self.scroll)
                            .size_full(),
                        )
                    })
                    .map(|el| match (&self.root, &self.root_error) {
                        (None, _) => el.child(
                            Placeholder::new(IconName::FolderOpen, "No folder open")
                                .compact()
                                .detail("Open a project to browse and edit its files here.")
                                .action(
                                    Button::new("empty-open-folder")
                                        .small()
                                        .primary()
                                        .label("Open Folder…")
                                        .on_click(
                                            cx.listener(|this, _, _, cx| this.prompt_open(cx)),
                                        ),
                                ),
                        ),
                        (Some(root), Some(e)) => {
                            let root = root.clone();
                            el.child(
                                Placeholder::error("Unable to read this folder")
                                    .compact()
                                    .detail(e.clone())
                                    .action(
                                        Button::new("retry-root")
                                            .small()
                                            .label("Try Again")
                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                this.set_root(root.clone(), cx)
                                            })),
                                    ),
                            )
                        }
                        (Some(root), None)
                            if self.children.get(root).is_some_and(|c| c.is_empty()) =>
                        {
                            el.child(
                                Placeholder::new(IconName::Folder, "This folder is empty")
                                    .compact()
                                    .detail("Files you add here show up automatically."),
                            )
                        }
                        _ => el,
                    }),
            )
    }
}

/// What the context menu can offer, read when the row is drawn.
#[derive(Clone, Copy)]
pub(super) struct MenuState {
    paste: bool,
    undo: bool,
    redo: bool,
    pub(super) git: bool,
}

fn file_menu(menu: PopupMenu, focus: FocusHandle, can: MenuState) -> PopupMenu {
    menu.action_context(focus)
        .menu("New File", Box::new(NewFile))
        .menu("New Folder", Box::new(NewFolder))
        .separator()
        .menu("Reveal in Finder", Box::new(RevealInFinder))
        .menu("Open in Default App", Box::new(OpenInDefaultApp))
        .menu("Open in Terminal", Box::new(OpenInTerminal))
        .separator()
        .menu("Find in Folder…", Box::new(FindInFolder))
        .separator()
        .menu("Cut", Box::new(Cut))
        .menu("Copy", Box::new(CopyEntry))
        .menu("Duplicate", Box::new(Duplicate))
        .menu_with_disabled("Paste", Box::new(Paste), !can.paste)
        .menu_with_disabled("Undo", Box::new(Undo), !can.undo)
        .menu_with_disabled("Redo", Box::new(Redo), !can.redo)
        .separator()
        .menu("Copy Path", Box::new(CopyPath))
        .menu("Copy Relative Path", Box::new(CopyRelativePath))
        .separator()
        .menu_with_disabled("Add to .gitignore", Box::new(AddToGitignore), !can.git)
        .menu_with_disabled(
            "Add to .git/info/exclude",
            Box::new(AddToGitExclude),
            !can.git,
        )
        .menu_with_disabled("View History", Box::new(ViewHistory), !can.git)
        .separator()
        .menu("Rename", Box::new(Rename))
        .menu("Trash", Box::new(Trash))
        .menu("Delete", Box::new(Delete))
        .separator()
        .menu("Expand All", Box::new(ExpandAll))
        .menu("Collapse All", Box::new(CollapseAll))
}
