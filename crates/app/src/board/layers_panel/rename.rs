//! A name typed in a row of the layers panel, a layer's or a page's: where the rename starts, the
//! field it is typed in, and how it ends, kept or dropped.

use figcraft_doc::NodeId;
use gpui_kit::component::Sizable as _;
use gpui_kit::component::input::{Escape, Input, InputEvent, InputState};
use gpui_kit::*;
use sawit_board::layers::LayerRow;

use super::{PageAction, RenameField, Renamed};
use crate::board::BoardView;

impl BoardView {
    /// Starts typing a new name in the row of the top one of `ids` that the tree lists: what
    /// `layers.rename` (⌘R, F2, Object ▸ Rename) and a double-click on a frame's name on the canvas
    /// (`canvas.renameLabel`) ask of the panel, and a double-click on a row does (ruling P4-C11).
    /// The panel shows, the layers holding `ids` open, the layer is selected alone and its row
    /// scrolled to, and its field takes the keyboard with the next frame. A name being typed in
    /// another row is kept first. With several ids only that one is renamed.
    pub(in crate::board) fn start_rename(&mut self, ids: &[NodeId], cx: &mut Context<Self>) {
        self.keep_typed_name(cx);
        self.show_layers = true;
        let open = self.reveal(ids);
        self.expanded.extend(open);
        let rows = self.layer_rows();
        let Some(id) = rows.iter().map(|row| row.id).find(|id| ids.contains(id)) else {
            cx.notify();
            return;
        };
        let selection = (self.board())
            .map(|board| board.session.selection())
            .unwrap_or_default();
        if selection != [id] {
            self.select_layers(vec![id], cx);
        }
        self.layer_anchor = Some(id);
        self.renaming = Some(id);
        self.layers.scroll_to = Some(id);
        cx.notify();
    }

    /// Starts typing a new name in page `id`'s row. A name being typed in another row is kept
    /// first.
    pub(super) fn start_page_rename(&mut self, id: NodeId, cx: &mut Context<Self>) {
        self.keep_typed_name(cx);
        self.renaming = None;
        self.layers.pages_closed = false;
        self.layers.renaming_page = Some(id);
        cx.notify();
    }

    /// Ends the rename going on, if any, keeping what was typed: another one starts, or a press
    /// on the canvas or a scrub of the Design panel is about to open a gesture that would take
    /// the name into it ([`BoardView::commit_focused_field`]). Its field leaves with it, and with
    /// the field its subscription, so the loss of the keyboard it hears after the next frame
    /// keeps nothing twice.
    pub(in crate::board) fn keep_typed_name(&mut self, cx: &mut Context<Self>) {
        if let Some(field) = self.layers.field.take() {
            let typed = field.input.read(cx).value().to_string();
            self.finish_rename(field.target, &typed, cx);
        }
    }

    /// Ends the rename of `target`, keeping what was typed: Enter in its field, or the field losing
    /// the keyboard. The canvas takes the keyboard back from the field that had it.
    fn commit_rename(&mut self, target: Renamed, window: &mut Window, cx: &mut Context<Self>) {
        let Some(field) = self.layers.field.take_if(|field| field.target == target) else {
            return;
        };
        let typed = field.input.read(cx).value().to_string();
        let had_keyboard = field.input.focus_handle(cx).is_focused(window);
        drop(field);
        self.finish_rename(target, &typed, cx);
        if had_keyboard {
            window.focus(&self.focus, cx);
        }
    }

    /// Ends the rename going on, dropping what was typed: Escape in its field. The canvas takes the
    /// keyboard back.
    fn cancel_rename(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(field) = self.layers.field.take() else {
            return;
        };
        self.end_rename_of(field.target);
        if field.input.focus_handle(cx).is_focused(window) {
            window.focus(&self.focus, cx);
        }
        cx.notify();
    }

    /// The rename of `target` is over, and `typed` is the name typed: the layer or the page takes
    /// it, unless it is blank or the name it has.
    fn finish_rename(&mut self, target: Renamed, typed: &str, cx: &mut Context<Self>) {
        self.end_rename_of(target);
        match target {
            Renamed::Layer(id) => self.rename_layer(id, typed, cx),
            Renamed::Page(id) => self.page_action(PageAction::Rename(id, typed.into()), cx),
        }
    }

    /// No name is being typed for `target` any more.
    fn end_rename_of(&mut self, target: Renamed) {
        match target {
            Renamed::Layer(id) if self.renaming == Some(id) => self.renaming = None,
            Renamed::Page(id) if self.layers.renaming_page == Some(id) => {
                self.layers.renaming_page = None;
            }
            Renamed::Layer(_) | Renamed::Page(_) => {}
        }
    }

    /// Makes the field for the rename that started, with the layer's or the page's name in it,
    /// selected, and the keyboard; drops the field of a rename that ended. The rename of a layer
    /// no longer listed, or of a page no longer there, ends as it is, as in FigCraft.
    pub(super) fn settle_rename_field(
        &mut self,
        rows: &[LayerRow],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let doc = self.board().and_then(|board| board.doc().ok());
        let named = |id: NodeId| {
            let node = doc.as_ref()?.node(id)?;
            Some(node.name.clone())
        };
        if let Some(id) = self.renaming
            && !rows.iter().any(|row| row.id == id)
        {
            self.renaming = None;
        }
        if let Some(id) = self.layers.renaming_page
            && !doc.as_ref().is_some_and(|doc| doc.pages.contains(&id))
        {
            self.layers.renaming_page = None;
        }
        let wanted =
            (self.renaming.map(Renamed::Layer)).or(self.layers.renaming_page.map(Renamed::Page));
        if self.layers.field.as_ref().map(|field| field.target) == wanted {
            return;
        }
        if let Some(old) = self.layers.field.take()
            && old.input.focus_handle(cx).is_focused(window)
        {
            window.focus(&self.focus, cx);
        }
        let Some(target) = wanted else {
            return;
        };
        let (Renamed::Layer(id) | Renamed::Page(id)) = target;
        let Some(name) = named(id) else {
            self.end_rename_of(target);
            return;
        };
        let input = cx.new(|cx| {
            let mut input = InputState::new(window, cx);
            input.set_value(name, window, cx);
            input
        });
        let events = cx.subscribe_in(
            &input,
            window,
            move |view, _, event: &InputEvent, window, cx| match event {
                InputEvent::PressEnter { .. } | InputEvent::Blur => {
                    view.commit_rename(target, window, cx);
                }
                InputEvent::Change | InputEvent::Focus => {}
            },
        );
        input.update(cx, |input, cx| {
            input.focus(window, cx);
            input.select_all(window, cx);
        });
        self.layers.field = Some(RenameField {
            target,
            input,
            _events: events,
        });
    }
}

/// The field a name is typed in, in place of a row's name. Escape in it drops what was typed;
/// a press in it stays its own, and doesn't select the row or start a drag.
pub(super) fn rename_field(input: &Entity<InputState>, cx: &mut Context<BoardView>) -> AnyElement {
    div()
        .debug_selector(|| "layers-rename".into())
        .flex_1()
        .min_w_0()
        .on_action(cx.listener(|view, _: &Escape, window, cx| view.cancel_rename(window, cx)))
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .child(Input::new(input).xsmall())
        .into_any_element()
}
