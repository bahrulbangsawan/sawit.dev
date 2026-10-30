//! The Design panel's number field ([`NumberField`]): a label beside an input for a number.
//!
//! - **Typing.** What is typed goes to every selected layer on Enter, or when the field loses the
//!   keyboard, through `sawit_board::design::parse_number`. Input it refuses changes nothing, and
//!   the field shows the layers' value again, as it does after an edit the engine refuses. The
//!   layers the value goes to are the ones on show when the field took the keyboard: a press on the
//!   canvas takes the keyboard back and may select others before the field hears of it. The text
//!   is selected when the field takes the keyboard, so what is typed replaces it.
//! - **Scrubbing.** The label is a drag handle: dragging it sideways changes the value by 1 per
//!   pixel (an opacity by 1%), ten times as fast with ⇧ held, and the whole drag is one undo step.
//!   Escape puts the value back ([`BoardView::scrub_begin`]). The drag's moves and its release are
//!   heard on the whole window ([`super::track_scrub`]).
//! - **Showing.** Layers whose values differ show "Mixed", and a value typed or scrubbed there goes
//!   to all of them. An opacity shows as a percentage, and takes one: a number typed without `%` is
//!   read as one (ruling P5-C7).

use std::rc::Rc;

use figcraft_doc::Document;
use figcraft_engine::guard;
use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use sawit_board::design::{DesignSelection, Mixed, parse_number};
use serde_json::{Value, json};

use super::Scrub;
use crate::board::BoardView;

/// A field's height.
const HEIGHT: f32 = 28.;

/// A number field of the Design panel, for property `key` (one that `node.set` takes), named
/// `label`: the label shows beside the input, or, for a field with an icon, as the icon's tooltip.
pub struct NumberField {
    key: &'static str,
    label: SharedString,
    icon: Option<Lucide>,
}

/// A number field's input and what the panel knows of it, kept from the first time it is drawn.
pub(super) struct Field {
    pub(super) input: Entity<InputState>,
    /// The text the field was last given from the layers: typed text that differs is applied.
    shown: SharedString,
    /// The layers the field edits while it has the keyboard: those on show when it took it.
    editing: Option<Rc<DesignSelection>>,
    _events: Subscription,
}

/// A scrub by a label's drag: where the pointer was last, in the window, how far the drag has gone
/// since the press, in pixels (ten for each one moved with ⇧ held), the value it began from, and
/// the value it set last.
pub(super) struct LabelDrag {
    last_x: f64,
    travel: f64,
    from: f64,
    value: f64,
}

impl NumberField {
    /// The field of property `key`, named `label`.
    pub fn new(key: &'static str, label: impl Into<SharedString>) -> Self {
        Self {
            key,
            label: label.into(),
            icon: None,
        }
    }

    /// Shows `icon` in place of the label, which becomes its tooltip.
    pub fn icon(mut self, icon: Lucide) -> Self {
        self.icon = Some(icon);
        self
    }

    /// The field as `view`'s panel draws it for the layers of `sel`. Its input shows their value,
    /// unless it has the keyboard; its label scrubs the value while there is one.
    pub(super) fn render(
        self,
        view: &mut BoardView,
        sel: &DesignSelection,
        window: &mut Window,
        cx: &mut Context<BoardView>,
    ) -> AnyElement {
        let Self { key, label, icon } = self;
        let value = sel.get(key);
        let input = view.field_input(key, window, cx);
        view.show_in_field(key, text(key, &value), window, cx);
        let scrubs = !matches!(value, Mixed::Absent);
        let theme = cx.theme();
        let (border, muted) = (theme.input, theme.muted_foreground);
        let handle = div()
            .id(SharedString::from(format!("design-handle-{key}")))
            .debug_selector(move || format!("design-handle-{key}"))
            .flex_shrink_0()
            .h_full()
            .min_w(px(22.))
            .px_1p5()
            .flex()
            .items_center()
            .justify_center()
            .text_xs()
            .text_color(muted)
            .map(|handle| match icon {
                Some(icon) => handle
                    .child(Icon::new(icon).xsmall())
                    .tooltip(move |window, cx| Tooltip::new(label.clone()).build(window, cx)),
                None => handle.child(label),
            })
            .when(scrubs, |handle| {
                handle.cursor(CursorStyle::ResizeLeftRight).on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |view, event: &MouseDownEvent, window, cx| {
                        // The press is the label's: nothing under it takes it.
                        cx.stop_propagation();
                        view.begin_label_scrub(key, event.position.x, window, cx);
                    }),
                )
            });
        h_flex()
            .flex_1()
            .min_w_0()
            .h(px(HEIGHT))
            .rounded(px(5.))
            .border_1()
            .border_color(border)
            .overflow_hidden()
            .child(handle)
            .child(
                div()
                    .debug_selector(move || format!("design-field-{key}"))
                    .flex_1()
                    .min_w_0()
                    .child(Input::new(&input).appearance(false).small()),
            )
            .into_any_element()
    }
}

/// How field `key` shows `value`: a number rounded to a hundredth ("100", "12.5"), an opacity as a
/// percentage ("50%"), "Mixed" when the layers differ or the value is a list (a padding or corner
/// radius whose sides differ), and nothing when no layer has the key.
pub(super) fn text(key: &str, value: &Mixed) -> SharedString {
    match value {
        Mixed::Absent => SharedString::default(),
        Mixed::Same(value) => match value.as_f64() {
            Some(opacity) if key == "opacity" => format!("{}%", (opacity * 100.0).round()).into(),
            Some(number) => format!("{}", (number * 100.0).round() / 100.0 + 0.0).into(),
            None => "Mixed".into(),
        },
        Mixed::Mixed => "Mixed".into(),
    }
}

/// The number typed `input` stands for in field `key`, or `None` when the field refuses it
/// ([`parse_number`]). An opacity is a percentage: one typed without its `%` gets one (ruling
/// P5-C7), so 50 is half.
pub(super) fn parse(key: &str, input: &str) -> Option<f64> {
    let input = input.trim();
    if key == "opacity" && !input.ends_with('%') {
        return parse_number(key, &format!("{input}%"));
    }
    parse_number(key, input)
}

/// How far a label's drag moves field `key` for each pixel: an opacity by 1%, any other number
/// by 1 (a rotation by a degree).
fn step(key: &str) -> f64 {
    if key == "opacity" { 0.01 } else { 1.0 }
}

/// `value` as a label's drag leaves field `key`: rounded to a hundredth, as the field shows it,
/// and held in the field's range by the rule typed input has ([`parse_number`]), so that a size,
/// gap, padding or radius stops at 0, and an opacity at 0 and at 100%.
fn scrubbed(key: &str, value: f64) -> f64 {
    let value = (value * 100.0).round() / 100.0;
    // A number below what the field takes is the only one `parse_number` refuses here.
    parse_number(key, &value.to_string()).unwrap_or(0.0)
}

/// A number that `value` holds: itself, or the largest of a list, as FigCraft's own panel starts a
/// scrub of corners that differ from the largest.
fn number(value: &Value) -> Option<f64> {
    value.as_f64().or_else(|| {
        let items = value.as_array()?;
        items.iter().filter_map(Value::as_f64).reduce(f64::max)
    })
}

/// Where a scrub of field `key` on the layers of `sel` starts: the value they share, or, when
/// they differ, the value of the first of them (in `doc`) that has one. `None` when none has.
fn start_value(sel: &DesignSelection, doc: &Document, key: &str) -> Option<f64> {
    match sel.get(key) {
        Mixed::Same(value) => number(&value),
        Mixed::Mixed => {
            sel.ids
                .iter()
                .find_map(|id| match DesignSelection::read(doc, &[*id]).get(key) {
                    Mixed::Same(value) => number(&value),
                    Mixed::Absent | Mixed::Mixed => None,
                })
        }
        Mixed::Absent => None,
    }
}

impl BoardView {
    /// The input of field `key`, made the first time the field is drawn: what it hears (it took
    /// the keyboard, Enter, it lost the keyboard) goes to the panel.
    fn field_input(
        &mut self,
        key: &'static str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<InputState> {
        if let Some(field) = self.design.fields.get(key) {
            return field.input.clone();
        }
        let input = cx.new(|cx| InputState::new(window, cx));
        let events = cx.subscribe_in(
            &input,
            window,
            move |view, _, event, window, cx| match event {
                InputEvent::Focus => view.field_focused(key, window, cx),
                InputEvent::PressEnter { .. } => view.commit_field(key, window, cx),
                InputEvent::Blur => {
                    view.commit_field(key, window, cx);
                    if let Some(field) = view.design.fields.get_mut(key) {
                        field.editing = None;
                    }
                }
                InputEvent::Change => {}
            },
        );
        self.design.fields.insert(
            key,
            Field {
                input: input.clone(),
                shown: SharedString::default(),
                editing: None,
                _events: events,
            },
        );
        input
    }

    /// Shows `text`, the layers' value, in field `key`, unless the field is being typed in: what
    /// is typed stays until it is applied. That lasts while the field has the keyboard, and until
    /// it hears that it lost it, which comes after the frame drawn once the keyboard has gone
    /// elsewhere: the typed text is applied then.
    fn show_in_field(
        &mut self,
        key: &'static str,
        text: SharedString,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(field) = self.design.fields.get_mut(key) else {
            return;
        };
        if field.editing.is_some() || field.input.focus_handle(cx).is_focused(window) {
            return;
        }
        if field.shown != text || field.input.read(cx).value() != text {
            field.shown = text.clone();
            field
                .input
                .update(cx, |input, cx| input.set_value(text, window, cx));
        }
    }

    /// Field `key` took the keyboard: it edits the layers on show now, and its text is selected,
    /// so that what is typed replaces it.
    fn field_focused(&mut self, key: &'static str, window: &mut Window, cx: &mut Context<Self>) {
        let sel = self.design_selection();
        let Some(field) = self.design.fields.get_mut(key) else {
            return;
        };
        field.editing = sel;
        field
            .input
            .update(cx, |input, cx| input.select_all(window, cx));
    }

    /// Applies what is typed in field `key` to the layers it edits, when that is another text than
    /// the field was given and a number the field takes. Then the field shows the layers' value:
    /// the one applied, or the one they keep when the input or the edit is refused.
    pub(super) fn commit_field(
        &mut self,
        key: &'static str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(field) = self.design.fields.get(key) else {
            return;
        };
        let typed = field.input.read(cx).value();
        if typed != field.shown {
            let editing = field.editing.clone();
            let target = editing.or_else(|| self.design_selection());
            if let (Some(value), Some(sel)) = (parse(key, &typed), target) {
                self.apply_edit(sel.set(key, json!(value)), cx);
            }
        }
        let now = self
            .design_selection()
            .map_or_else(SharedString::default, |sel| text(key, &sel.get(key)));
        if let Some(field) = self.design.fields.get_mut(key) {
            field.shown = now.clone();
            field
                .input
                .update(cx, |input, cx| input.set_value(now, window, cx));
        }
    }

    /// Applies what is typed in the field that has the keyboard, if one has.
    fn commit_focused_field(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let focused = (self.design.fields.iter())
            .find(|(_, field)| field.input.focus_handle(cx).is_focused(window))
            .map(|(key, _)| *key);
        if let Some(key) = focused {
            self.commit_field(key, window, cx);
        }
    }

    /// The left button went down on field `key`'s label, at `x` in the window: a scrub by the
    /// label's drag starts there, from the layers' value ([`start_value`]). A field being typed in
    /// is applied first, and the canvas takes the keyboard, so that the drag's Escape comes to it
    /// ([`BoardView::on_key`]).
    fn begin_label_scrub(
        &mut self,
        key: &'static str,
        x: Pixels,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.commit_focused_field(window, cx);
        window.focus(&self.focus, cx);
        let Some(sel) = self.design_selection() else {
            return;
        };
        let Some(doc) = self.board().and_then(|board| board.doc().ok()) else {
            return;
        };
        let from = guard::run("ui.scrub", || Ok(start_value(&sel, &doc, key)));
        let Some(from) = from.ok().flatten() else {
            return;
        };
        self.scrub_begin(key, cx);
        if let Some(scrub) = &mut self.scrub {
            scrub.drag = Some(LabelDrag {
                last_x: f64::from(x),
                travel: 0.0,
                from,
                // What a move too small to count would set: no edit until the value changes.
                value: scrubbed(key, from),
            });
        }
    }

    /// The pointer moved, anywhere: while a label's drag scrubs a field, its value follows the
    /// pointer sideways, by the field's step for each pixel ([`step`]), ten times as fast while ⇧
    /// is held. A move without the left button means the drag's release never came: the scrub
    /// ends, kept.
    pub(super) fn scrub_drag(&mut self, event: &MouseMoveEvent, cx: &mut Context<Self>) {
        let held = event.pressed_button == Some(MouseButton::Left);
        let Some(Scrub {
            key,
            drag: Some(drag),
            ..
        }) = &mut self.scrub
        else {
            return;
        };
        if !held {
            self.scrub_end(true, cx);
            return;
        }
        let x = f64::from(event.position.x);
        let rate = if event.modifiers.shift { 10.0 } else { 1.0 };
        drag.travel += (x - drag.last_x) * rate;
        drag.last_x = x;
        let value = scrubbed(key, drag.from + drag.travel.round() * step(key));
        if value == drag.value {
            return;
        }
        drag.value = value;
        let key = *key;
        self.scrub_to(key, value, cx);
    }

    /// The left button came up, or went down, anywhere: a label's scrub that is on ends, kept.
    pub(super) fn end_label_scrub(&mut self, cx: &mut Context<Self>) {
        if self.scrubbing_label() {
            self.scrub_end(true, cx);
        }
    }
}

#[cfg(test)]
mod tests {
    use sawit_board::design::Mixed;
    use serde_json::json;

    // Not `super::*`: gpui-kit's `test` attribute would come with it, in place of the standard one.
    use super::{number, parse, scrubbed, step, text};

    #[test]
    fn fields_show_numbers_percentages_and_mixed() {
        let same = |value| Mixed::Same(value);
        assert_eq!(text("x", &same(json!(100.0))).as_ref(), "100");
        assert_eq!(text("x", &same(json!(12.5))).as_ref(), "12.5");
        assert_eq!(text("x", &same(json!(-0.001))).as_ref(), "0");
        assert_eq!(text("width", &same(json!(33.333_333))).as_ref(), "33.33");
        // An opacity as the engine reads it back, an f32 widened.
        let opacity = f64::from(0.3_f32);
        assert_eq!(text("opacity", &same(json!(opacity))).as_ref(), "30%");
        assert_eq!(text("x", &Mixed::Mixed).as_ref(), "Mixed");
        // Sides that differ.
        assert_eq!(
            text("padding", &same(json!([1.0, 2.0, 3.0, 4.0]))).as_ref(),
            "Mixed"
        );
        assert_eq!(text("gap", &Mixed::Absent).as_ref(), "");
    }

    #[test]
    fn typed_input_is_read_as_parse_number_reads_it() {
        assert_eq!(parse("x", " -5 "), Some(-5.0));
        assert_eq!(parse("width", "-5"), None);
        assert_eq!(parse("x", "abc"), None);
        assert_eq!(parse("x", "Mixed"), None);
        assert_eq!(parse("x", ""), None);
        // An opacity is a percentage, with its % or without (ruling P5-C7).
        assert_eq!(parse("opacity", "50"), Some(0.5));
        assert_eq!(parse("opacity", "50%"), Some(0.5));
        assert_eq!(parse("opacity", "300"), Some(1.0));
        assert_eq!(parse("opacity", "abc"), None);
    }

    #[test]
    fn a_scrub_stays_in_the_fields_range() {
        assert_eq!(scrubbed("x", -3.0), -3.0);
        assert_eq!(scrubbed("rotation", -45.0), -45.0);
        // Sizes, gaps, paddings and radii stop at 0.
        for key in [
            "width",
            "gap",
            "paddingTop",
            "topLeftRadius",
            "cornerRadius",
        ] {
            assert_eq!(scrubbed(key, -3.0), 0.0, "{key}");
        }
        assert_eq!(scrubbed("opacity", 1.2), 1.0);
        assert_eq!(scrubbed("opacity", -0.2), 0.0);
        // Rounded to a hundredth, as the field shows it.
        assert_eq!(scrubbed("opacity", f64::from(0.3_f32) + 0.05), 0.35);
        assert_eq!(step("opacity"), 0.01);
        assert_eq!(step("x"), 1.0);
    }

    #[test]
    fn a_list_scrubs_from_its_largest() {
        assert_eq!(number(&json!(4.0)), Some(4.0));
        assert_eq!(number(&json!([1.0, 8.0, 3.0, 2.0])), Some(8.0));
        assert_eq!(number(&json!("auto")), None);
    }
}
