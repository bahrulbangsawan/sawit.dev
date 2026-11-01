//! The name box of New File, New Folder and Rename, typed in the tree's own row.

use std::path::{Path, PathBuf};

use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::{AppContext as _, Context, Window};

use super::{After, Edit, EditKind, FilesPanel, Named, Op, Request};

impl FilesPanel {
    pub(super) fn start_new(&mut self, dir: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(parent) = self.target_dir() else {
            return;
        };
        self.expand_to(&parent, cx);
        self.begin_edit(EditKind::New { dir }, parent, "", window, cx);
    }

    pub(super) fn start_rename(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(path) = self.selection() else {
            return;
        };
        let (Some(parent), Some(name)) = (path.parent(), path.file_name()) else {
            return;
        };
        let (parent, name) = (parent.to_path_buf(), name.to_string_lossy().into_owned());
        self.begin_edit(EditKind::Rename(path), parent, &name, window, cx);
    }

    fn begin_edit(
        &mut self,
        kind: EditKind,
        parent: PathBuf,
        name: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Rename selects the name without its extension.
        let stem = match &kind {
            EditKind::Rename(path) if !path.is_dir() => {
                Path::new(name).file_stem().map_or(name.len(), |s| s.len())
            }
            _ => name.len(),
        };
        let input = cx.new(|cx| {
            let mut state = InputState::new(window, cx);
            state.set_value(name.to_string(), window, cx);
            state.set_selected_range(0..stem, cx);
            state
        });
        let subscription = cx.subscribe_in(
            &input,
            window,
            |this, _, e: &InputEvent, window, cx| match e {
                InputEvent::PressEnter { .. } => this.confirm_edit(window, cx),
                InputEvent::Blur => this.cancel_edit(window, cx),
                _ => {}
            },
        );
        input.update(cx, |s, cx| s.focus(window, cx));
        self.error = None;
        self.edit = Some(Edit {
            kind,
            parent,
            input,
            _subscription: subscription,
        });
        cx.notify();
    }

    pub(super) fn cancel_edit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.edit.take().is_some() {
            window.focus(&self.focus, cx);
            cx.notify();
        }
    }

    fn confirm_edit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(edit) = &self.edit else {
            return;
        };
        let name = edit.input.read(cx).value().trim().to_string();
        if name.contains('/') || name == "." || name == ".." {
            self.error = Some(format!("“{name}” can't be used as a name."));
            cx.notify();
            return;
        }
        let path = edit.parent.join(&name);
        let op = match &edit.kind {
            _ if name.is_empty() => None,
            EditKind::New { dir } => Some(Op::Create { path, dir: *dir }),
            EditKind::Rename(from) if *from == path => None,
            EditKind::Rename(from) => Some(Op::Move {
                from: from.clone(),
                to: path,
            }),
        };
        let named = Named {
            kind: edit.kind.clone(),
            parent: edit.parent.clone(),
            name,
        };
        self.cancel_edit(window, cx);
        if let Some(op) = op {
            let after = After {
                named: Some(named),
                ..After::default()
            };
            self.request(Request::Do(op, after), window, cx);
        }
    }

    /// Opens the name box again with the name typed for a change that failed, unless a name
    /// is being typed again already or the box would be drawn nowhere.
    pub(super) fn retype(&mut self, named: Named, window: &mut Window, cx: &mut Context<Self>) {
        let drawn = match &named.kind {
            EditKind::Rename(path) => self.shown(path),
            EditKind::New { .. } => {
                Some(&named.parent) == self.root.as_ref()
                    || self.shown(&named.parent) && self.expanded.contains(&named.parent)
            }
        };
        if self.edit.is_none() && drawn {
            self.begin_edit(named.kind, named.parent, &named.name, window, cx);
        }
    }
}
