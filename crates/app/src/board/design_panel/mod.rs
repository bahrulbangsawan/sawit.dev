//! The Design panel (spec "Design panel", D11): a column [`WIDTH`] wide on the canvas's right that
//! shows what the selected layers hold and changes it, in the spec's sections and order
//! (`sawit_board::design::Section`): the header, then Position ([`position`]), Auto layout
//! ([`layout`]), Appearance ([`appearance`]), Fill and Stroke ([`paints`]), Effects ([`effects`]),
//! Typography ([`typography`]) and Export ([`export`]).
//!
//! - **Reading.** The panel reads the selection through `sawit_board::design`
//!   ([`DesignSelection`]): once for each document and selection, behind the engine's guard, so a
//!   frame drawn for anything else reads nothing, and a panic in the read costs the panel its
//!   fields, not the app (D15). The models drawn from it (the paints, the effects, the fonts) are
//!   built behind the guard too ([`guarded`](controls::guarded)).
//! - **Editing.** Every change is the model's [`Edit`], run through [`BoardView::apply_edit`]: one
//!   undo step for all the selected layers, and the engine's refusal as a toast. An edit of a list
//!   (the fills, the effects, the export settings) is made from the layers as they are when it is
//!   made ([`BoardView::edit_layers`]), since it sends the whole list back. A number field
//!   ([`number_field::NumberField`]) takes a typed number, and scrubs when its label is dragged:
//!   a scrub is one undo step however many values it went through, and Escape puts the value
//!   back ([`BoardView::scrub_begin`]). The colours a colour picker goes through while it is open
//!   are a scrub of their own ([`paints`]).
//! - **Showing.** The panel shows at first beside a design file and not beside a board (spec
//!   D11). `view.design` (⌥8, and the top bar's toggle) shows or hides it, through the engine's
//!   `panel.design` request. While the UI is hidden (`⌘\`) it is not drawn, and it comes back with
//!   the UI (ruling P5-C18). With nothing selected it says so, and offers no field.
//! - **The keyboard.** A field has it while it is typed in. Every other control leaves the canvas's
//!   shortcuts working (delegate ruling on P5.T4): a click on a button or a menu gives the keyboard
//!   to the canvas, a colour picker gives it back to where it was when it closes, and the dashed
//!   switch and the font family list, which take it for themselves, give it to the canvas once
//!   they are done.

mod appearance;
mod controls;
mod effects;
mod export;
mod layout;
mod number_field;
mod paints;
mod position;
mod scrub;
mod typography;

use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::sync::Arc;

use figcraft_doc::{Document, NodeId};
use figcraft_engine::guard;
#[cfg(test)]
use gpui_kit::assets::IconName as Lucide;
use gpui_kit::base::actions::Cancel;
use gpui_kit::component::*;
use gpui_kit::*;
use sawit_board::design::{DesignSelection, Edit, Mixed, Section};
use serde_json::Value;

pub(super) use scrub::track_scrub;

use super::BoardView;
use number_field::{Field, LabelDrag};

/// The panel's width (spec D11).
pub(super) const WIDTH: f32 = 264.;

/// What the Design panel keeps from one frame to the next.
#[derive(Default)]
pub(super) struct DesignPanel {
    /// The selection as last read, with the document and the layers it was read from.
    read: Option<Read>,
    /// The fields' inputs, by the field's name (a number field's is the key it edits), each made
    /// the first time it is drawn.
    fields: HashMap<SharedString, Field>,
    /// The Appearance section shows the radius of each corner on its own.
    corners_open: bool,
    /// The Auto layout section shows the padding of each side on its own.
    padding_open: bool,
    /// The colour pickers of the paint and shadow rows, by name, each made the first time it is
    /// drawn.
    pickers: HashMap<SharedString, paints::Picker>,
    /// The colour picker whose colours make the scrub that is on.
    colour: Option<SharedString>,
    /// The frame being drawn drew the swatch of that colour picker
    /// ([`BoardView::settle_colour_scrub`]).
    colour_drawn: bool,
    /// The effects whose editor is open, by their place in the list, and the layers they are the
    /// effects of: the editors close when other layers are selected.
    effects_open: HashSet<usize>,
    effects_of: Vec<NodeId>,
    /// The Typography section's font family list, made the first time it is drawn.
    families: Option<typography::Families>,
}

/// The selection as the panel last read it.
struct Read {
    /// The document it was read from, kept so that no other one can take its address.
    doc: Arc<Document>,
    ids: Vec<NodeId>,
    /// What the layers hold, or `None` when reading them panicked.
    sel: Option<Rc<DesignSelection>>,
}

/// The scrub of a field that is on ([`BoardView::scrub_begin`]): the key it changes, the layers
/// it changes it on, and, for a scrub by a label's drag, where that drag is.
pub(super) struct Scrub {
    key: &'static str,
    sel: Rc<DesignSelection>,
    drag: Option<LabelDrag>,
}

impl BoardView {
    /// Runs `edit`, one of the Design panel's, as a click on the chrome runs a command
    /// ([`BoardView::choose`], ruling P5-C20): the engine runs it as one undo step, the canvas
    /// carries out what the engine asks of it, and a refusal shows as a toast.
    pub fn apply_edit(&mut self, edit: Edit, cx: &mut Context<Self>) {
        self.choose(edit.command, edit.params, cx);
    }

    /// The selected layers as the Design panel shows them. They are read again only when the
    /// document or the selection is another than the last time, behind the engine's guard: a
    /// panic costs the panel its fields (`None`), not the app (D15). `None` without a document.
    fn design_selection(&mut self) -> Option<Rc<DesignSelection>> {
        let ds = self.board()?.session.ds().ok()?;
        if let Some(read) = &self.design.read
            && Arc::ptr_eq(&read.doc, &ds.doc)
            && read.ids == ds.selection
        {
            return read.sel.clone();
        }
        let (doc, ids) = (ds.doc.clone(), ds.selection.clone());
        let sel = guard::run("ui.design", || Ok(DesignSelection::read(&doc, &ids)))
            .ok()
            .map(Rc::new);
        self.design.read = Some(Read {
            doc,
            ids,
            sel: sel.clone(),
        });
        sel
    }

    /// Makes an edit of the selected layers with `make`, from the layers as the document holds
    /// them now, and runs it ([`BoardView::apply_edit`]). An edit of a list (the paints, the
    /// effects, the export settings) sends the whole list back, so it is made from what the list
    /// holds when it is made, not from what the panel showed. `make` gives `None` for an edit that
    /// can't be made, and then nothing runs.
    pub(super) fn edit_layers(
        &mut self,
        make: impl FnOnce(&DesignSelection) -> Option<Edit>,
        cx: &mut Context<Self>,
    ) {
        let Some(sel) = self.design_selection() else {
            return;
        };
        if let Ok(Some(edit)) = guard::run("ui.design", || Ok(make(&sel))) {
            self.apply_edit(edit, cx);
        }
    }
}

/// The Design panel of `view`'s canvas: the header, then the sections for what is selected, or a
/// line that says nothing is.
pub(super) fn render_design_panel(
    view: &mut BoardView,
    window: &mut Window,
    cx: &mut Context<BoardView>,
) -> impl IntoElement + use<> {
    let theme = cx.theme();
    let panel = v_flex()
        .id("board-design-panel")
        .debug_selector(|| "board-design-panel".into())
        .flex_shrink_0()
        .w(px(WIDTH))
        .h_full()
        .overflow_y_scroll()
        .border_l_1()
        .border_color(theme.border)
        .bg(theme.background)
        .text_color(theme.foreground)
        .text_sm()
        // Escape in an open colour picker: heard before the picker closes on it, so that what
        // the picker changed goes back instead of staying (ruling P5-C15).
        .capture_action(cx.listener(|view, _: &Cancel, _, cx| view.cancel_colour_scrub(cx)));
    let muted = theme.muted_foreground;
    let Some(sel) = view.design_selection().filter(|sel| !sel.ids.is_empty()) else {
        return panel.child(
            div()
                .debug_selector(|| "design-nothing-selected".into())
                .px_3()
                .py_4()
                .text_color(muted)
                .child(NOTHING_SELECTED),
        );
    };
    let mut panel = panel.child(header(&sel, cx));
    for section in sel.sections() {
        let shown = match section {
            // Drawn above.
            Section::Header => None,
            Section::Position => Some(position::render(view, &sel, window, cx)),
            Section::AutoLayout => Some(layout::render(view, &sel, window, cx)),
            Section::Appearance => Some(appearance::render(view, &sel, window, cx)),
            Section::Fill => Some(paints::render_fill(view, &sel, window, cx)),
            Section::Stroke => Some(paints::render_stroke(view, &sel, window, cx)),
            Section::Effects => Some(effects::render(view, &sel, window, cx)),
            Section::Typography => Some(typography::render(view, &sel, window, cx)),
            Section::Export => Some(export::render(&sel, cx)),
        };
        panel = panel.children(shown);
    }
    panel
}

/// What the panel says while nothing is selected.
const NOTHING_SELECTED: &str = "Select a layer to see its design.";

/// What the header says of `sel`: the kind of its one layer ("Rectangle"), or how many layers it
/// has ("3 layers"), and the one layer's name (ruling P5-C16).
fn header_text(sel: &DesignSelection) -> (String, Option<String>) {
    let name = match (sel.ids.len(), sel.get("name")) {
        (1, Mixed::Same(Value::String(name))) => Some(name),
        _ => None,
    };
    (sel.header(), name)
}

/// The panel's header: the kind of the one layer selected and its name, or how many layers are.
fn header(sel: &DesignSelection, cx: &App) -> Div {
    let theme = cx.theme();
    let (kind, name) = header_text(sel);
    h_flex()
        .debug_selector(|| "design-header".into())
        .w_full()
        .h(px(40.))
        .px_3()
        .gap_2()
        .border_b_1()
        .border_color(theme.border)
        .child(
            div()
                .flex_shrink_0()
                .text_color(theme.muted_foreground)
                .child(kind),
        )
        .children(name.map(|name| div().min_w_0().font_medium().truncate().child(name)))
}

/// Every icon the panel shows, for the test that they are all embedded (ruling P5-C11).
#[cfg(test)]
const ICONS: &[Lucide] = &[
    position::ROTATION_ICON,
    position::LINKED_ICON,
    position::UNLINKED_ICON,
    appearance::OPACITY_ICON,
    appearance::RADIUS_ICON,
    appearance::CORNERS_ICON,
    appearance::BLEND_ICON,
    appearance::SHOWN_ICON,
    appearance::HIDDEN_ICON,
    appearance::MIXED_SHOWN_ICON,
    layout::ADD_ICON,
    layout::REMOVE_ICON,
    layout::VERTICAL_ICON,
    layout::HORIZONTAL_ICON,
    layout::WRAP_ICON,
    layout::GAP_ACROSS_ICON,
    layout::GAP_DOWN_ICON,
    layout::PADDING_ICON,
    layout::SIDES_ICON,
    paints::ADD_ICON,
    paints::REMOVE_ICON,
    paints::REPLACE_ICON,
    paints::IMAGE_ICON,
    paints::PATTERN_ICON,
    effects::EDIT_ICON,
];

#[cfg(test)]
mod tests;
