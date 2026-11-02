//! What is typed in a field: its input, made the first time it is drawn; the layers' value shown
//! in it while it isn't typed in; and the typed text applied on Enter, or when the field loses the
//! keyboard, or before a press or a scrub would take it in.

use std::rc::Rc;

use figcraft_doc::NodeId;
use figcraft_engine::guard;
use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::*;
use sawit_board::design::DesignSelection;

use super::super::controls::guarded;
use super::{Field, Rule};
use crate::board::BoardView;

impl BoardView {
    /// The input of field `id`, made the first time the field is drawn: what it hears (it took
    /// the keyboard, Enter, it lost the keyboard) goes to the panel. The field does what `rule`
    /// says, until it is drawn with another.
    pub(super) fn field_input(
        &mut self,
        id: SharedString,
        rule: Rule,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<InputState> {
        if let Some(field) = self.design.fields.get_mut(&id) {
            field.rule = rule;
            return field.input.clone();
        }
        let input = cx.new(|cx| InputState::new(window, cx));
        let events = cx.subscribe_in(&input, window, {
            let id = id.clone();
            move |view, _, event, window, cx| match event {
                InputEvent::Focus => view.field_focused(&id, window, cx),
                InputEvent::PressEnter { .. } => view.commit_field(&id, window, cx),
                InputEvent::Blur => {
                    view.commit_field(&id, window, cx);
                    if let Some(field) = view.design.fields.get_mut(&id) {
                        field.editing = None;
                    }
                }
                InputEvent::Change => {}
            }
        });
        self.design.fields.insert(
            id,
            Field {
                input: input.clone(),
                shown: SharedString::default(),
                editing: None,
                rule,
                _events: events,
            },
        );
        input
    }

    /// Shows `text`, the layers' value, in field `id`, unless the field is being typed in: what
    /// is typed stays until it is applied. That lasts while the field has the keyboard, and until
    /// it hears that it lost it, which comes after the frame drawn once the keyboard has gone
    /// elsewhere: the typed text is applied then.
    pub(super) fn show_in_field(
        &mut self,
        id: &str,
        text: SharedString,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(field) = self.design.fields.get_mut(id) else {
            return;
        };
        if field.editing.is_some() || field.input.focus_handle(cx).is_focused(window) {
            return;
        }
        // The input's text read in place: `value` would copy it out for each field each frame.
        if field.shown != text || *field.input.read(cx).text() != *text {
            field.shown = text.clone();
            field
                .input
                .update(cx, |input, cx| input.set_value(text, window, cx));
        }
    }

    /// Field `id` took the keyboard: it edits the layers on show now, and its text is selected,
    /// so that what is typed replaces it.
    fn field_focused(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        let sel = self.design_selection();
        let Some(field) = self.design.fields.get_mut(id) else {
            return;
        };
        field.editing = sel;
        field
            .input
            .update(cx, |input, cx| input.select_all(window, cx));
    }

    /// Applies what is typed in field `id` to the layers it edits, as they are now, when that is
    /// another text than the field was given and one its rule takes. Then the field shows the
    /// layers' value: the one applied, or the one they keep when the input or the edit is refused.
    pub(in crate::board::design_panel) fn commit_field(
        &mut self,
        id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(field) = self.design.fields.get(id) else {
            return;
        };
        let typed = field.input.read(cx).value();
        let rule = field.rule.clone();
        if typed != field.shown {
            let ids = field.editing.as_ref().map(|sel| sel.ids.clone());
            if let Some(sel) = self.layers_now(ids.as_deref())
                && let Some(edit) = guarded("ui.design", None, || (rule.apply)(&sel, &typed))
            {
                self.apply_edit(edit, cx);
            }
        }
        let now = self
            .design_selection()
            .map_or_else(SharedString::default, |sel| {
                guarded("ui.design", SharedString::default(), || (rule.show)(&sel))
            });
        if let Some(field) = self.design.fields.get_mut(id) {
            field.shown = now.clone();
            field
                .input
                .update(cx, |input, cx| input.set_value(now, window, cx));
        }
    }

    /// Layers `ids` as the document holds them now, or the selected layers for `None`. They are
    /// read behind the engine's guard, as the panel reads the selection.
    fn layers_now(&mut self, ids: Option<&[NodeId]>) -> Option<Rc<DesignSelection>> {
        let shown = self.design_selection()?;
        let Some(ids) = ids.filter(|ids| *ids != shown.ids.as_slice()) else {
            return Some(shown);
        };
        let doc = self.board()?.doc().ok()?;
        let read = guard::run("ui.design", || Ok(DesignSelection::read(&doc, ids)));
        read.ok().map(Rc::new)
    }

    /// Applies what is typed in the field being typed in, if one is: the one that has the
    /// keyboard, or one that had it and hasn't heard yet that it lost it (it hears that after the
    /// next frame). A scrub applies it before it begins: else it would be applied within the
    /// scrub, as part of its undo step, and an Escape would take it back with the scrub. A press
    /// on the canvas applies it before the tools have the press, for the same reason
    /// ([`BoardView::on_left_down`]).
    ///
    /// A name being typed in a row of the layers panel, a layer's or a page's, is kept first, for
    /// the same reason: its field too hears only after the next frame that it lost the keyboard,
    /// and the gesture or the scrub begun meanwhile took the name back (CANVAS-UI-1).
    pub(in crate::board) fn commit_focused_field(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.keep_typed_name(cx);
        let typed: Vec<SharedString> = (self.design.fields.iter())
            .filter(|(_, field)| {
                field.editing.is_some() || field.input.focus_handle(cx).is_focused(window)
            })
            .map(|(id, _)| id.clone())
            .collect();
        for id in typed {
            self.commit_field(&id, window, cx);
        }
    }
}
