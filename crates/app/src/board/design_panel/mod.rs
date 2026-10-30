//! The Design panel (spec "Design panel", D11): a column [`WIDTH`] wide on the canvas's right that
//! shows what the selected layers hold and changes it, in the spec's sections and order
//! (`sawit_board::design::Section`): the header, then Position ([`position`]), Auto layout
//! ([`layout`]) and Appearance ([`appearance`]). Fill, Stroke, Effects, Typography and Export come
//! with Plan 5 Task 5; until then their sections show nothing.
//!
//! - **Reading.** The panel reads the selection through `sawit_board::design`
//!   ([`DesignSelection`]): once for each document and selection, behind the engine's guard, so a
//!   frame drawn for anything else reads nothing, and a panic in the read costs the panel its
//!   fields, not the app (D15).
//! - **Editing.** Every change is the model's [`Edit`], run through [`BoardView::apply_edit`]: one
//!   undo step for all the selected layers, and the engine's refusal as a toast. A number field
//!   ([`number_field::NumberField`]) takes a typed number, and scrubs when its label is dragged:
//!   a scrub is one undo step however many values it went through, and Escape puts the value
//!   back ([`BoardView::scrub_begin`]).
//! - **Showing.** The panel shows at first beside a design file and not beside a board (spec
//!   D11). `view.design` (⌥8, and the top bar's toggle) shows or hides it, through the engine's
//!   `panel.design` request. While the UI is hidden (`⌘\`) it is not drawn, and it comes back with
//!   the UI (ruling P5-C18). With nothing selected it says so, and offers no field.

mod appearance;
mod layout;
mod number_field;
mod position;

use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

use figcraft_doc::{Document, NodeId};
use figcraft_engine::guard;
use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::*;
use gpui_kit::component::*;
use gpui_kit::*;
use sawit_board::BoardError;
use sawit_board::design::{DesignSelection, Edit, Mixed, Section};
use serde_json::{Value, json};

use super::BoardView;
use number_field::{Field, LabelDrag};

/// The panel's width (spec D11).
pub(super) const WIDTH: f32 = 264.;

/// What the Design panel keeps from one frame to the next.
#[derive(Default)]
pub(super) struct DesignPanel {
    /// The selection as last read, with the document and the layers it was read from.
    read: Option<Read>,
    /// The number fields' inputs, by the key they edit, each made the first time it is drawn.
    fields: HashMap<&'static str, Field>,
    /// The Appearance section shows the radius of each corner on its own.
    corners_open: bool,
    /// The Auto layout section shows the padding of each side on its own.
    padding_open: bool,
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

    /// Starts a scrub of field `key` on the selected layers: the engine opens an interaction
    /// (`Session::begin_interaction`), and the values [`scrub_to`](Self::scrub_to) sets until
    /// [`scrub_end`](Self::scrub_end) are previews within it, which end as one undo step or none.
    /// A scrub still on ends first, kept, rather than take this one into its undo step. The
    /// engine's refusal (a version on show in place of the document) shows as a toast, and no
    /// scrub starts.
    pub fn scrub_begin(&mut self, key: &'static str, cx: &mut Context<Self>) {
        if self.scrub.is_some() {
            self.scrub_end(true, cx);
        }
        let Some(sel) = self.design_selection() else {
            return;
        };
        let begun = self.run(
            |board| {
                let session = &mut board.session;
                guard::run("ui.scrub", || session.begin_interaction(key)).map_err(BoardError::from)
            },
            cx,
        );
        if begun.is_some() {
            self.scrub = Some(Scrub {
                key,
                sel,
                drag: None,
            });
        }
    }

    /// Sets field `key` to `value` on the layers of the scrub that is on: a `node.set` within its
    /// interaction, which the canvas shows at once and which makes no undo step of its own. Without
    /// a scrub on it does nothing.
    pub fn scrub_to(&mut self, key: &'static str, value: f64, cx: &mut Context<Self>) {
        let Some(scrub) = &self.scrub else {
            return;
        };
        let edit = scrub.sel.set(key, json!(value));
        self.apply_edit(edit, cx);
    }

    /// Ends the scrub that is on. With `commit`, what it set stays, as one undo step
    /// (`Session::commit_interaction`; none when nothing changed). Without, the layers go back to
    /// how they were when it began, and no undo step is made (`Session::cancel_interaction`).
    pub fn scrub_end(&mut self, commit: bool, cx: &mut Context<Self>) {
        if self.scrub.take().is_none() {
            return;
        }
        self.run(
            |board| {
                let session = &mut board.session;
                guard::run("ui.scrub", || {
                    if commit {
                        session.commit_interaction()
                    } else {
                        session.cancel_interaction()
                    }
                })
                .map_err(BoardError::from)
            },
            cx,
        );
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

    /// Whether a label's drag is scrubbing a field: it takes the keys until it ends
    /// ([`BoardView::on_key`]).
    pub(super) fn scrubbing_label(&self) -> bool {
        self.scrub
            .as_ref()
            .is_some_and(|scrub| scrub.drag.is_some())
    }
}

/// Registers, for the frame being painted, the listeners on the whole window that give a label's
/// scrub its moves and its end wherever the pointer goes, as the canvas's drags get theirs
/// (`events::track_drags`, ruling P5-C14): a move changes the value, and the left button's release
/// ends the scrub, kept. A press, or a move without the button, means the release never came: it
/// ends the scrub too. They listen in the capture phase, before anything under the pointer can
/// stop the events.
pub(super) fn track_scrub(view: &WeakEntity<BoardView>, window: &mut Window) {
    window.on_mouse_event({
        let view = view.clone();
        move |event: &MouseMoveEvent, phase, _, cx| {
            if phase == DispatchPhase::Capture {
                let _ = view.update(cx, |view, cx| view.scrub_drag(event, cx));
            }
        }
    });
    window.on_mouse_event({
        let view = view.clone();
        move |event: &MouseUpEvent, phase, _, cx| {
            if phase == DispatchPhase::Capture && event.button == MouseButton::Left {
                let _ = view.update(cx, |view, cx| view.end_label_scrub(cx));
            }
        }
    });
    window.on_mouse_event({
        let view = view.clone();
        move |_: &MouseDownEvent, phase, _, cx| {
            if phase == DispatchPhase::Capture {
                let _ = view.update(cx, |view, cx| view.end_label_scrub(cx));
            }
        }
    });
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
        .text_sm();
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
            Section::Position => Some(position::render(view, &sel, window, cx)),
            Section::AutoLayout => Some(layout::render(view, &sel, window, cx)),
            Section::Appearance => Some(appearance::render(view, &sel, window, cx)),
            // The header is drawn above; the other sections are Plan 5 Task 5's.
            Section::Header
            | Section::Fill
            | Section::Stroke
            | Section::Effects
            | Section::Typography
            | Section::Export => None,
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

/// A section of the panel: `title`, with `actions` (small buttons) on its right, over `rows`, and
/// a line under it all.
fn section(
    id: &'static str,
    title: &'static str,
    actions: Vec<AnyElement>,
    rows: Vec<AnyElement>,
    cx: &App,
) -> AnyElement {
    v_flex()
        .debug_selector(move || format!("design-section-{id}"))
        .w_full()
        .px_3()
        .pt_2()
        .pb_3()
        .gap_2()
        .border_b_1()
        .border_color(cx.theme().border)
        .child(
            h_flex()
                .w_full()
                .h(px(24.))
                .justify_between()
                .child(div().font_medium().child(title))
                .child(h_flex().gap_1().children(actions)),
        )
        .children(rows)
        .into_any_element()
}

/// A row of a section: fields and buttons side by side.
fn row() -> Div {
    h_flex().w_full().gap_2()
}

/// A small button showing `icon` alone, for a section's toggles and actions.
fn icon_button(id: impl Into<ElementId>, icon: Lucide, tooltip: impl Into<SharedString>) -> Button {
    Button::new(id).ghost().xsmall().icon(icon).tooltip(tooltip)
}

/// The edit that sets several properties at once, `props`, on every selected layer, as
/// [`DesignSelection::set`] sets one that is not a text's.
fn set_props(sel: &DesignSelection, props: Value) -> Edit {
    Edit {
        command: "node.set",
        params: json!({"ids": sel.ids, "props": props}),
    }
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
];

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use figcraft_doc::NodeId;
    use gpui_kit::{
        AppContext as _, AssetSource as _, Entity, Modifiers, MouseButton, TestAppContext,
        VisualTestContext, point, px, size,
    };
    use sawit_board::design::layout::align_cell;
    use sawit_board::design::{DesignSelection, Edit, Mixed};
    use serde_json::{Value, json};

    use super::super::{BoardView, top_bar};
    use super::{ICONS, NOTHING_SELECTED, WIDTH, appearance, header_text, position};

    /// A folder of the test's own, removed when the test ends.
    struct Scratch(PathBuf);

    impl Scratch {
        fn new(test: &str) -> Self {
            let dir = std::env::temp_dir()
                .join(format!("sawit-design-panel-{}-{test}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// A design file being made, before it is written.
    struct Design(figcraft_engine::Session);

    impl Design {
        fn new() -> Self {
            Self(figcraft_engine::Session::new())
        }

        /// Makes a layer with `node.create`'s params `layer`, and returns its id.
        fn make(&mut self, layer: Value) -> NodeId {
            let made = self.0.execute("node.create", &layer).unwrap();
            NodeId(made["id"].as_u64().unwrap())
        }

        /// Sets `props` on layers `ids`.
        fn set(&mut self, ids: &[NodeId], props: Value) {
            let params = json!({"ids": ids, "props": props});
            self.0.execute("node.set", &params).unwrap();
        }

        /// Writes the file at `path`.
        fn save(mut self, path: &Path) {
            let params = json!({"path": path.to_str().unwrap()});
            self.0.execute("file.saveAs", &params).unwrap();
        }
    }

    /// Writes a design file at `path` with a layer made from each of `layers` (`node.create`'s
    /// params) on its page, and returns their ids.
    fn design_file(path: &Path, layers: &[Value]) -> Vec<NodeId> {
        let mut design = Design::new();
        let ids = layers.iter().map(|layer| design.make(layer.clone()));
        let ids = ids.collect();
        design.save(path);
        ids
    }

    /// A 100 × 100 rectangle at (`x`, 0).
    fn rectangle(x: f64) -> Value {
        json!({"type": "rectangle", "x": x, "y": 0, "width": 100, "height": 100})
    }

    /// A 200 × 100 frame at (`x`, 0) holding a 20 × 20 rectangle, and that rectangle.
    fn frame_with_a_child(design: &mut Design, x: f64) -> (NodeId, NodeId) {
        let frame = json!({"type": "frame", "x": x, "y": 0, "width": 200, "height": 100});
        let frame = design.make(frame);
        let child = json!({"type": "rectangle", "parent": frame, "width": 20, "height": 20});
        (frame, design.make(child))
    }

    /// The canvas of the file at `path` in a window of its own, as the workspace shows it, with
    /// `ids` selected and drawn. The window is tall enough for every section to show at once, and
    /// has the keyboard, as the one the user types in does: its fields hear when they take the
    /// keyboard and lose it.
    fn open_in_window(
        cx: &mut TestAppContext,
        path: &Path,
        ids: &[NodeId],
    ) -> (Entity<BoardView>, VisualTestContext) {
        cx.executor().allow_parking();
        cx.update(gpui_kit::init);
        let path = path.to_path_buf();
        let window = cx.open_window(size(px(900.), px(1000.)), |_, cx| BoardView::open(path, cx));
        let view = window.root(cx).unwrap();
        let mut cx = VisualTestContext::from_window(window.into(), cx);
        cx.update(|window, _| window.activate_window());
        select(&view, ids, &mut cx);
        (view, cx)
    }

    /// Selects `ids`, as a click or an agent does, and draws the window again.
    fn select(view: &Entity<BoardView>, ids: &[NodeId], cx: &mut VisualTestContext) {
        view.update(cx, |view, cx| {
            let board = view.board_mut().unwrap();
            board.session.set_selection(ids.to_vec());
            cx.notify();
        });
        cx.run_until_parked();
    }

    /// Clicks the middle of what the window drew with debug selector `selector`.
    fn click(cx: &mut VisualTestContext, selector: &'static str) {
        let drawn = cx.debug_bounds(selector);
        let bounds = drawn.unwrap_or_else(|| panic!("{selector} isn't drawn"));
        cx.simulate_click(bounds.center(), Modifiers::default());
        cx.run_until_parked();
    }

    /// Whether the window drew something with debug selector `selector`.
    fn drawn(cx: &mut VisualTestContext, selector: &'static str) -> bool {
        cx.debug_bounds(selector).is_some()
    }

    /// What field `key` reads.
    fn field_text(view: &Entity<BoardView>, key: &str, cx: &TestAppContext) -> String {
        view.read_with(cx, |view, cx| {
            view.design.fields[key].input.read(cx).value().to_string()
        })
    }

    /// Gives the canvas the keyboard, as a press on it does.
    fn focus_canvas(view: &Entity<BoardView>, cx: &mut VisualTestContext) {
        cx.update(|window, cx| {
            let focus = view.read(cx).focus.clone();
            window.focus(&focus, cx);
        });
        cx.run_until_parked();
    }

    /// The selected layers' `key`, as the Design panel reads it.
    fn panel_value(
        view: &Entity<BoardView>,
        ids: &[NodeId],
        key: &str,
        cx: &TestAppContext,
    ) -> Mixed {
        view.read_with(cx, |view, _| {
            let doc = view.board().unwrap().doc().unwrap();
            DesignSelection::read(&doc, ids).get(key)
        })
    }

    /// The canvas of the design file at `path`, opened as the workspace opens it, with `ids`
    /// selected.
    fn open(cx: &mut TestAppContext, path: &Path, ids: &[NodeId]) -> Entity<BoardView> {
        cx.executor().allow_parking();
        cx.update(gpui_kit::init);
        let view = cx.new(|cx| BoardView::open(path.to_path_buf(), cx));
        view.update(cx, |view, _| {
            let board = view.board_mut().unwrap();
            board.session.set_selection(ids.to_vec());
        });
        view
    }

    /// Layer `id`'s `key`, as the engine's inspector gives it.
    fn prop(view: &Entity<BoardView>, id: NodeId, key: &str, cx: &TestAppContext) -> Value {
        view.read_with(cx, |view, _| {
            let doc = view.board().unwrap().doc().unwrap();
            figcraft_engine::inspect::node_json(&doc, id, 0)[key].clone()
        })
    }

    /// How many undo steps the canvas's file has.
    fn undo_steps(view: &Entity<BoardView>, cx: &TestAppContext) -> usize {
        view.read_with(cx, |view, _| view.board().unwrap().session.undo_depth().0)
    }

    #[gpui_kit::test]
    fn scrub_is_one_undo_step(cx: &mut TestAppContext) {
        let tmp = Scratch::new("scrub-one-step");
        let path = tmp.0.join("design.figcraft");
        let ids = design_file(&path, &[rectangle(0.0)]);
        let view = open(cx, &path, &ids);
        let steps = undo_steps(&view, cx);
        view.update(cx, |view, cx| {
            view.scrub_begin("x", cx);
            for step in 1..=20 {
                view.scrub_to("x", f64::from(step) * 5.0, cx);
            }
            view.scrub_end(true, cx);
        });
        assert_eq!(prop(&view, ids[0], "x", cx), json!(100.0));
        assert_eq!(undo_steps(&view, cx), steps + 1);
        // One undo takes the whole scrub back.
        view.update(cx, |view, cx| view.choose("edit.undo", json!({}), cx));
        assert_eq!(prop(&view, ids[0], "x", cx), json!(0.0));
        assert_eq!(undo_steps(&view, cx), steps);
    }

    #[gpui_kit::test]
    fn cancelled_scrub_restores(cx: &mut TestAppContext) {
        let tmp = Scratch::new("scrub-cancel");
        let path = tmp.0.join("design.figcraft");
        let ids = design_file(&path, &[rectangle(0.0)]);
        let view = open(cx, &path, &ids);
        let steps = undo_steps(&view, cx);
        view.update(cx, |view, cx| {
            view.scrub_begin("x", cx);
            view.scrub_to("x", 30.0, cx);
            view.scrub_to("x", 60.0, cx);
        });
        // The canvas shows each value as it comes.
        assert_eq!(prop(&view, ids[0], "x", cx), json!(60.0));
        view.update(cx, |view, cx| view.scrub_end(false, cx));
        assert_eq!(prop(&view, ids[0], "x", cx), json!(0.0));
        assert_eq!(undo_steps(&view, cx), steps);
    }

    #[gpui_kit::test]
    fn apply_edit_error_sets_toast(cx: &mut TestAppContext) {
        let tmp = Scratch::new("apply-edit-error");
        let path = tmp.0.join("design.figcraft");
        let ids = design_file(&path, &[rectangle(0.0)]);
        let view = open(cx, &path, &ids);
        let edit = Edit {
            command: "no.such.command",
            params: json!({}),
        };
        view.update(cx, |view, cx| view.apply_edit(edit, cx));
        assert!(view.read_with(cx, |view, _| view.toast.is_some()));
    }

    /// Ruling P5-C3: a key typed in a field goes to the field, and the canvas's keys come back
    /// with the keyboard.
    #[gpui_kit::test]
    fn typing_in_a_field_leaves_the_canvas_keys_alone(cx: &mut TestAppContext) {
        let tmp = Scratch::new("type-in-field");
        let path = tmp.0.join("design.figcraft");
        let ids = design_file(&path, &[rectangle(0.0)]);
        let (view, mut cx) = open_in_window(cx, &path, &ids);
        let tool = |cx: &VisualTestContext| {
            view.read_with(cx, |view, _| view.board().unwrap().session.tool.tool.id())
        };
        let before = tool(&cx);
        assert_eq!(field_text(&view, "x", &cx), "0");
        // A click in X gives the field the keyboard, its text selected: what is typed replaces it.
        click(&mut cx, "design-field-x");
        cx.simulate_keystrokes("r");
        assert_eq!(tool(&cx), before);
        assert_eq!(field_text(&view, "x", &cx), "r");
        // "r" is no number: Enter changes nothing, and the field shows the value again.
        let steps = undo_steps(&view, &cx);
        cx.simulate_keystrokes("enter");
        assert_eq!(field_text(&view, "x", &cx), "0");
        assert_eq!(prop(&view, ids[0], "x", &cx), json!(0.0));
        assert_eq!(undo_steps(&view, &cx), steps);
        assert_eq!(view.read_with(&cx, |view, _| view.toast.clone()), None);
        // A number is applied, as one undo step.
        cx.simulate_keystrokes("cmd-a 4 0 enter");
        assert_eq!(prop(&view, ids[0], "x", &cx), json!(40.0));
        assert_eq!(undo_steps(&view, &cx), steps + 1);
        assert_eq!(field_text(&view, "x", &cx), "40");
        // Back on the canvas, its keys are its own again.
        focus_canvas(&view, &mut cx);
        cx.simulate_keystrokes("r");
        assert_eq!(tool(&cx), "rectangle");
    }

    /// Review Focus 1 through the panel: a value typed over "Mixed" goes to every layer, as one
    /// undo step. Ruling P5-C7: an opacity typed without % is a percentage.
    #[gpui_kit::test]
    fn a_value_typed_over_mixed_goes_to_every_layer(cx: &mut TestAppContext) {
        let tmp = Scratch::new("type-mixed");
        let path = tmp.0.join("design.figcraft");
        let ids = design_file(&path, &[rectangle(0.0), rectangle(150.0)]);
        let (view, mut cx) = open_in_window(cx, &path, &ids);
        assert_eq!(field_text(&view, "x", &cx), "Mixed");
        assert_eq!(field_text(&view, "width", &cx), "100");
        let steps = undo_steps(&view, &cx);
        click(&mut cx, "design-field-x");
        cx.simulate_keystrokes("1 0 enter");
        for id in &ids {
            assert_eq!(prop(&view, *id, "x", &cx), json!(10.0));
        }
        assert_eq!(undo_steps(&view, &cx), steps + 1);
        assert_eq!(field_text(&view, "x", &cx), "10");
        // 50 in the opacity field is half.
        assert_eq!(field_text(&view, "opacity", &cx), "100%");
        click(&mut cx, "design-field-opacity");
        cx.simulate_keystrokes("5 0 enter");
        assert_eq!(
            panel_value(&view, &ids, "opacity", &cx),
            Mixed::Same(json!(0.5))
        );
        assert_eq!(field_text(&view, "opacity", &cx), "50%");
    }

    /// A press on the canvas after typing in a field: the typed value goes to the layers the
    /// field was showing, not to the ones the press selects.
    #[gpui_kit::test]
    fn a_field_applies_to_the_layers_it_showed(cx: &mut TestAppContext) {
        let tmp = Scratch::new("field-target");
        let path = tmp.0.join("design.figcraft");
        let ids = design_file(&path, &[rectangle(0.0), rectangle(150.0)]);
        let (view, mut cx) = open_in_window(cx, &path, &ids[..1]);
        click(&mut cx, "design-field-width");
        cx.simulate_keystrokes("6 0");
        // Another layer is selected, and the canvas takes the keyboard back: the field loses it.
        select(&view, &ids[1..], &mut cx);
        focus_canvas(&view, &mut cx);
        assert_eq!(prop(&view, ids[0], "width", &cx), json!(60.0));
        assert_eq!(prop(&view, ids[1], "width", &cx), json!(100.0));
        // The field shows the layer on show now.
        assert_eq!(field_text(&view, "width", &cx), "100");
    }

    /// The scrub of a label's drag, from the press to a release anywhere (ruling P5-C14): 1 per
    /// pixel, ten times as fast with ⇧, one undo step; an opacity moves 1% a pixel (P5-C7).
    #[gpui_kit::test]
    fn a_label_drag_scrubs_as_one_undo_step(cx: &mut TestAppContext) {
        let tmp = Scratch::new("label-drag");
        let path = tmp.0.join("design.figcraft");
        let ids = design_file(&path, &[rectangle(0.0)]);
        let (view, mut cx) = open_in_window(cx, &path, &ids);
        let steps = undo_steps(&view, &cx);
        let (left, none) = (MouseButton::Left, Modifiers::default());
        let shift = Modifiers {
            shift: true,
            ..Modifiers::default()
        };
        let start = cx.debug_bounds("design-handle-x").unwrap().center();
        cx.simulate_mouse_down(start, left, none);
        cx.simulate_mouse_move(start + point(px(10.), px(0.)), left, none);
        assert_eq!(prop(&view, ids[0], "x", &cx), json!(10.0));
        cx.simulate_mouse_move(start + point(px(12.), px(4.)), left, shift);
        assert_eq!(prop(&view, ids[0], "x", &cx), json!(30.0));
        // Released over the canvas, far from the label: the scrub ends there, as one undo step.
        cx.simulate_mouse_up(point(px(100.), px(300.)), left, none);
        assert!(view.read_with(&cx, |view, _| view.scrub.is_none()));
        assert_eq!(prop(&view, ids[0], "x", &cx), json!(30.0));
        assert_eq!(undo_steps(&view, &cx), steps + 1);
        // The press selected nothing on the canvas, and drew nothing there.
        let selected = view.read_with(&cx, |view, _| view.board().unwrap().session.selection());
        assert_eq!(selected, ids);
        // An opacity moves 1% a pixel.
        let start = cx.debug_bounds("design-handle-opacity").unwrap().center();
        cx.simulate_mouse_down(start, left, none);
        cx.simulate_mouse_move(start - point(px(25.), px(0.)), left, none);
        cx.simulate_mouse_up(start - point(px(25.), px(0.)), left, none);
        assert_eq!(
            panel_value(&view, &ids, "opacity", &cx),
            Mixed::Same(json!(0.75))
        );
        assert_eq!(undo_steps(&view, &cx), steps + 2);
        view.update(&mut cx, |view, cx| view.choose("edit.undo", json!({}), cx));
        view.update(&mut cx, |view, cx| view.choose("edit.undo", json!({}), cx));
        assert_eq!(prop(&view, ids[0], "x", &cx), json!(0.0));
    }

    /// Ruling P5-C3: Escape during a label's scrub puts the value back, before the canvas can
    /// take the key.
    #[gpui_kit::test]
    fn escape_puts_a_label_scrub_back(cx: &mut TestAppContext) {
        let tmp = Scratch::new("label-escape");
        let path = tmp.0.join("design.figcraft");
        let ids = design_file(&path, &[rectangle(0.0)]);
        let (view, mut cx) = open_in_window(cx, &path, &ids);
        let steps = undo_steps(&view, &cx);
        let (left, none) = (MouseButton::Left, Modifiers::default());
        let start = cx.debug_bounds("design-handle-x").unwrap().center();
        let moved = start + point(px(30.), px(0.));
        cx.simulate_mouse_down(start, left, none);
        cx.simulate_mouse_move(moved, left, none);
        assert_eq!(prop(&view, ids[0], "x", &cx), json!(30.0));
        cx.simulate_keystrokes("escape");
        assert_eq!(prop(&view, ids[0], "x", &cx), json!(0.0));
        assert!(view.read_with(&cx, |view, _| view.scrub.is_none()));
        // The release that follows changes nothing more.
        cx.simulate_mouse_move(moved + point(px(20.), px(0.)), left, none);
        cx.simulate_mouse_up(moved, left, none);
        assert_eq!(prop(&view, ids[0], "x", &cx), json!(0.0));
        assert_eq!(undo_steps(&view, &cx), steps);
        // The canvas didn't see Escape, which would have selected nothing.
        let selected = view.read_with(&cx, |view, _| view.board().unwrap().session.selection());
        assert_eq!(selected, ids);
    }

    /// Ruling P5-C18, and the defaults of spec D11: the panel shows beside a design file, goes
    /// with the UI, and comes back with it.
    #[gpui_kit::test]
    fn the_design_panel_goes_with_the_ui_and_comes_back(cx: &mut TestAppContext) {
        let tmp = Scratch::new("panel-ui");
        let path = tmp.0.join("design.figcraft");
        let ids = design_file(&path, &[rectangle(0.0)]);
        let (view, mut cx) = open_in_window(cx, &path, &ids);
        let shown = |cx: &VisualTestContext| view.read_with(cx, |view, _| view.show_design);
        let canvas_width = |cx: &VisualTestContext| {
            view.read_with(cx, |view, _| f32::from(view.bounds.size.width))
        };
        assert!(shown(&cx));
        assert!(drawn(&mut cx, "board-design-panel"));
        assert_eq!(canvas_width(&cx), 900. - WIDTH);
        let toggle_ui = |cx: &mut VisualTestContext| {
            view.update(cx, |view, cx| view.choose("view.toggleUi", json!({}), cx));
            cx.run_until_parked();
        };
        toggle_ui(&mut cx);
        assert!(!drawn(&mut cx, "board-design-panel"));
        assert!(shown(&cx));
        assert_eq!(canvas_width(&cx), 900.);
        toggle_ui(&mut cx);
        assert!(drawn(&mut cx, "board-design-panel"));
        assert_eq!(canvas_width(&cx), 900. - WIDTH);
    }

    /// `view.design` shows and hides the panel: from an agent's command, and from the top bar's
    /// toggle. A board opens without it (spec D11).
    #[gpui_kit::test]
    fn view_design_shows_and_hides_the_panel(cx: &mut TestAppContext) {
        let tmp = Scratch::new("panel-toggle");
        let (view, mut cx) = open_in_window(cx, &tmp.0.join("canvas.figcraft"), &[]);
        assert!(!view.read_with(&cx, |view, _| view.show_design));
        assert!(!drawn(&mut cx, "board-design-panel"));
        let op = json!({"op": "command", "command": "view.design"});
        let reply = view.update(&mut cx, |view, cx| view.run_op(&op, cx));
        assert_eq!(reply["ok"], json!(true), "{reply}");
        cx.run_until_parked();
        assert!(drawn(&mut cx, "board-design-panel"));
        // With nothing selected, it says so, and has no field.
        assert!(drawn(&mut cx, "design-nothing-selected"));
        assert!(!drawn(&mut cx, "design-section-position"));
        click(&mut cx, "board-design");
        assert!(!drawn(&mut cx, "board-design-panel"));
        click(&mut cx, "board-design");
        assert!(drawn(&mut cx, "board-design-panel"));
        assert_eq!(NOTHING_SELECTED, "Select a layer to see its design.");
    }

    /// Ruling P5.T3 (carried): one edit aligns frames that run both ways, each along its own
    /// direction, and one undo puts both back.
    #[gpui_kit::test]
    fn aligning_frames_that_run_both_ways_is_one_undo_step(cx: &mut TestAppContext) {
        let tmp = Scratch::new("align-both-ways");
        let path = tmp.0.join("design.figcraft");
        let mut design = Design::new();
        let (across, _) = frame_with_a_child(&mut design, 0.0);
        let (down, _) = frame_with_a_child(&mut design, 400.0);
        design.set(&[across], json!({"layout": "horizontal"}));
        design.set(&[down], json!({"layout": "vertical"}));
        design.save(&path);
        let ids = [across, down];
        let view = open(cx, &path, &ids);
        let aligned = |cx: &TestAppContext| {
            [across, down].map(|frame| {
                let main = panel_value(&view, &[frame], "mainAlign", cx);
                let cross = panel_value(&view, &[frame], "crossAlign", cx);
                (main, cross)
            })
        };
        let before = aligned(cx);
        let steps = undo_steps(&view, cx);
        let sel = view.read_with(cx, |view, _| {
            DesignSelection::read(&view.board().unwrap().doc().unwrap(), &ids)
        });
        view.update(cx, |view, cx| view.apply_edit(align_cell(&sel, 0, 2), cx));
        assert_eq!(view.read_with(cx, |view, _| view.toast.clone()), None);
        let same = |align: &str| Mixed::Same(json!(align));
        // The top right cell: along the columns for the horizontal frame, down the rows for the
        // vertical one.
        assert_eq!(
            aligned(cx),
            [(same("max"), same("min")), (same("min"), same("max"))]
        );
        assert_eq!(undo_steps(&view, cx), steps + 1);
        view.update(cx, |view, cx| view.choose("edit.undo", json!({}), cx));
        assert_eq!(aligned(cx), before);
    }

    /// The Auto layout section: adding keeps the selection, the direction sets layout and wrap
    /// together, a cell of the grid aligns, sizing comes from its menu, and removing takes the
    /// layout off.
    #[gpui_kit::test]
    fn the_auto_layout_section_edits_the_frames(cx: &mut TestAppContext) {
        let tmp = Scratch::new("auto-layout");
        let path = tmp.0.join("design.figcraft");
        let mut design = Design::new();
        let (plain, _) = frame_with_a_child(&mut design, 0.0);
        let (laid_out, _) = frame_with_a_child(&mut design, 400.0);
        design.set(&[laid_out], json!({"layout": "horizontal"}));
        design.save(&path);
        let ids = [plain, laid_out];
        let (view, mut cx) = open_in_window(cx, &path, &ids);
        let steps = undo_steps(&view, &cx);
        let value = |key: &str, cx: &VisualTestContext| panel_value(&view, &ids, key, cx);
        assert_eq!(value("layout", &cx), Mixed::Mixed);
        assert!(drawn(&mut cx, "design-layout-remove"));
        click(&mut cx, "design-layout-add");
        let added = panel_value(&view, &[plain], "layout", &cx);
        assert!(matches!(added, Mixed::Same(_)), "{added:?}");
        let selected = view.read_with(&cx, |view, _| view.board().unwrap().session.selection());
        assert_eq!(selected, ids);
        assert_eq!(undo_steps(&view, &cx), steps + 1);
        assert!(!drawn(&mut cx, "design-layout-add"));

        click(&mut cx, "design-layout-vertical");
        assert_eq!(value("layout", &cx), Mixed::Same(json!("vertical")));
        click(&mut cx, "design-layout-wrap");
        assert_eq!(value("layout", &cx), Mixed::Same(json!("horizontal")));
        assert_eq!(value("wrap", &cx), Mixed::Same(json!(true)));
        click(&mut cx, "design-layout-vertical");
        assert_eq!(value("wrap", &cx), Mixed::Same(json!(false)));
        assert_eq!(undo_steps(&view, &cx), steps + 4);

        // Vertical: the bottom middle cell is the end of the column, centred across it.
        click(&mut cx, "design-align-cell-2-1");
        assert_eq!(value("mainAlign", &cx), Mixed::Same(json!("max")));
        assert_eq!(value("crossAlign", &cx), Mixed::Same(json!("center")));

        click(&mut cx, "design-sizingH");
        click(&mut cx, "design-sizingH-hug");
        assert_eq!(value("sizingH", &cx), Mixed::Same(json!("hug")));

        // Each side's padding on its own.
        click(&mut cx, "design-padding-sides");
        click(&mut cx, "design-field-paddingTop");
        cx.simulate_keystrokes("1 2 enter");
        assert_eq!(value("paddingTop", &cx), Mixed::Same(json!(12.0)));
        assert_eq!(field_text(&view, "padding", &cx), "Mixed");

        click(&mut cx, "design-layout-remove");
        assert_eq!(value("layout", &cx), Mixed::Absent);
        assert!(drawn(&mut cx, "design-layout-add"));
        assert!(!drawn(&mut cx, "design-align-grid"));
    }

    /// The Appearance section: the eye (ruling P5-C17), each corner's radius on its own, and the
    /// blend mode.
    #[gpui_kit::test]
    fn the_appearance_section_edits_the_layers(cx: &mut TestAppContext) {
        let tmp = Scratch::new("appearance");
        let path = tmp.0.join("design.figcraft");
        let ids = design_file(&path, &[rectangle(0.0), rectangle(150.0)]);
        let (view, mut cx) = open_in_window(cx, &path, &ids);
        let value = |key: &str, cx: &VisualTestContext| panel_value(&view, &ids, key, cx);
        click(&mut cx, "design-visible");
        assert_eq!(value("visible", &cx), Mixed::Same(json!(false)));
        click(&mut cx, "design-visible");
        assert_eq!(value("visible", &cx), Mixed::Same(json!(true)));
        // One hidden and one shown: the eye shows all.
        view.update(&mut cx, |view, cx| {
            let hide = json!({"ids": [ids[0]], "props": {"visible": false}});
            view.choose("node.set", hide, cx);
        });
        cx.run_until_parked();
        assert_eq!(value("visible", &cx), Mixed::Mixed);
        click(&mut cx, "design-visible");
        assert_eq!(value("visible", &cx), Mixed::Same(json!(true)));

        click(&mut cx, "design-corners");
        click(&mut cx, "design-field-topLeftRadius");
        cx.simulate_keystrokes("8 enter");
        assert_eq!(value("topLeftRadius", &cx), Mixed::Same(json!(8.0)));
        assert_eq!(value("topRightRadius", &cx), Mixed::Same(json!(0.0)));
        assert_eq!(field_text(&view, "cornerRadius", &cx), "Mixed");

        click(&mut cx, "design-blend");
        click(&mut cx, "design-blend-Multiply");
        assert_eq!(value("blend", &cx), Mixed::Same(json!("multiply")));
        assert_eq!(view.read_with(&cx, |view, _| view.toast.clone()), None);
    }

    #[test]
    fn the_eye_shows_what_a_click_does() {
        let (shown, _, hides) = appearance::visibility(&Mixed::Same(json!(true)));
        assert_eq!((shown, hides), (appearance::SHOWN_ICON, false));
        let (hidden, _, shows) = appearance::visibility(&Mixed::Same(json!(false)));
        assert_eq!((hidden, shows), (appearance::HIDDEN_ICON, true));
        let (mixed, _, shows_all) = appearance::visibility(&Mixed::Mixed);
        assert_eq!((mixed, shows_all), (appearance::MIXED_SHOWN_ICON, true));
    }

    /// Ruling P5-C16: the header is the kind and the name of one layer, or the count of several.
    #[test]
    fn the_header_names_the_kind_and_the_layer() {
        let mut design = Design::new();
        let card =
            design.make(json!({"type": "rectangle", "name": "Card", "width": 10, "height": 10}));
        let other = design.make(rectangle(50.0));
        let doc = design.0.ds().unwrap().doc.clone();
        let header = |ids: &[NodeId]| header_text(&DesignSelection::read(&doc, ids));
        assert_eq!(header(&[card]), ("Rectangle".into(), Some("Card".into())));
        assert_eq!(header(&[card, other]), ("2 layers".into(), None));
    }

    /// Ruling P5-C11: every icon the panel and its toggle show is one Sawit embeds.
    #[test]
    fn every_icon_of_the_design_panel_is_embedded() {
        let icons = ICONS.iter().copied().chain(position::align_icons());
        for icon in icons.chain([top_bar::DESIGN_ICON]) {
            let path = icon.path();
            let svg = crate::assets::AppAssets.load(&path).unwrap();
            assert!(svg.is_some(), "{path} isn't embedded");
        }
    }
}
