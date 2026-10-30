//! The Fill and Stroke sections (spec "Design panel" 5 and 6): the layers' paints, the top one
//! first as in FigCraft's own panel, and for the strokes their weight, position and dashes.
//!
//! - A solid paint is a swatch, its hex and its opacity. The swatch opens the colour picker
//!   ([`colour_swatch`], which the shadows of the Effects section share): the colours it goes
//!   through while it is open are one scrub of the list, one undo step when the picker closes, and
//!   none when Escape closes it (ruling P5-C15).
//! - A gradient, an image or a pattern shows what it is, and becomes a solid only when its
//!   "Replace with solid" button is clicked (Review Focus 4).
//! - Every paint has its eye and a button that removes it. Layers whose paints differ show
//!   "Mixed", and its "Replace all" gives them all one new paint, as the section's add button
//!   does then.
//!
//! The paints change only through `sawit_board::design::paints`, which sends a list back with
//! the rows it doesn't change as they were (delegate ruling on P5.T2); the stroke's weight,
//! position and dashes are plain properties.

use std::rc::Rc;

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::color_picker::{ColorPicker, ColorPickerEvent, ColorPickerState};
use gpui_kit::component::switch::Switch;
use gpui_kit::component::*;
use gpui_kit::*;
use sawit_board::design::effects::{EffectKind, effects, set_effect};
use sawit_board::design::paints::{
    PaintKind, PaintRow, add_paint, paints, parse_hex, remove_paint, replace_with_solid, set_solid,
    set_visible,
};
use sawit_board::design::{DesignSelection, Edit, Mixed};
use serde_json::{Value, json};

use super::number_field::{self, NumberField, Rule, TextField};
use super::{
    Choices, choice_menu, eye_button, guarded, icon_button, mixed_row, on_click_edit, row,
    row_button, section,
};
use crate::board::BoardView;

/// The picture of the buttons that add a paint or an effect, and of those that remove one.
pub(super) const ADD_ICON: Lucide = Lucide::Plus;
pub(super) const REMOVE_ICON: Lucide = Lucide::Minus;
/// The picture of a gradient, image or pattern row's "Replace with solid" button.
pub(super) const REPLACE_ICON: Lucide = Lucide::Replace;
/// What an image paint and a pattern paint show in place of a swatch.
pub(super) const IMAGE_ICON: Lucide = Lucide::Image;
pub(super) const PATTERN_ICON: Lucide = Lucide::Shapes;

/// The width of an opacity field, beside a hex.
pub(super) const OPACITY_WIDTH: f32 = 56.;
/// The side of a swatch.
const SWATCH: f32 = 20.;

/// A dashed stroke's dashes: 4 drawn, 4 left out.
const DASHES: [f64; 2] = [4.0, 4.0];

/// Where a stroke can sit on the layer's edge: what `node.set` takes, and what the menu says.
const STROKE_ALIGNS: [(&str, &str); 3] = [
    ("inside", "Inside"),
    ("center", "Center"),
    ("outside", "Outside"),
];

/// The Fill section for the layers of `sel`.
pub(super) fn render_fill(
    view: &mut BoardView,
    sel: &Rc<DesignSelection>,
    window: &mut Window,
    cx: &mut Context<BoardView>,
) -> AnyElement {
    let this = cx.entity().downgrade();
    let rows = paint_rows(view, sel, "fills", window, cx);
    let add = add_button("fills", "Add fill", &this);
    section("fill", "Fill", vec![add], rows, cx)
}

/// The Stroke section for the layers of `sel`: their stroke paints, then, while they have a
/// stroke, its weight, its position and whether it is dashed.
pub(super) fn render_stroke(
    view: &mut BoardView,
    sel: &Rc<DesignSelection>,
    window: &mut Window,
    cx: &mut Context<BoardView>,
) -> AnyElement {
    let this = cx.entity().downgrade();
    let mut rows = paint_rows(view, sel, "strokes", window, cx);
    if has_strokes(sel) {
        let weight = NumberField::new("strokeWeight", "Weight").render(view, sel, window, cx);
        let align = stroke_align(sel, &this);
        rows.push(row().child(weight).child(align).into_any_element());
        rows.push(dashed(sel, &this));
    }
    let add = add_button("strokes", "Add stroke", &this);
    section("stroke", "Stroke", vec![add], rows, cx)
}

/// The button that adds a solid paint to list `key` of every layer: after their paints, or, when
/// they differ, as the only one.
fn add_button(
    key: &'static str,
    tooltip: &'static str,
    view: &WeakEntity<BoardView>,
) -> AnyElement {
    let selector = format!("design-{key}-add");
    icon_button(SharedString::from(selector.clone()), ADD_ICON, tooltip)
        .debug_selector(move || selector)
        .on_click(on_click_edit(view, move |sel| Some(add_paint(sel, key))))
        .into_any_element()
}

/// Whether any of the layers of `sel` has a stroke.
fn has_strokes(sel: &DesignSelection) -> bool {
    match sel.get("strokes") {
        Mixed::Same(Value::Array(strokes)) => !strokes.is_empty(),
        Mixed::Mixed => true,
        Mixed::Same(_) | Mixed::Absent => false,
    }
}

/// The rows of list `key` of the layers of `sel`, the top paint first; "Mixed" when the layers
/// hold different lists.
fn paint_rows(
    view: &mut BoardView,
    sel: &Rc<DesignSelection>,
    key: &'static str,
    window: &mut Window,
    cx: &mut Context<BoardView>,
) -> Vec<AnyElement> {
    let this = cx.entity().downgrade();
    let Some(rows) = guarded("ui.paints", Some(Vec::new()), || paints(sel, key)) else {
        let replace = move |sel: &DesignSelection| Some(add_paint(sel, key));
        return vec![mixed_row(key, &this, replace, cx)];
    };
    let mut drawn = Vec::with_capacity(rows.len());
    for (index, paint) in rows.iter().enumerate().rev() {
        drawn.push(paint_row(view, sel, key, index, paint, window, cx));
    }
    drawn
}

/// Row `index` of list `key`: a solid's swatch, hex and opacity, or what a gradient, an image or a
/// pattern is with its "Replace with solid" button; then the eye and the remove button.
fn paint_row(
    view: &mut BoardView,
    sel: &DesignSelection,
    key: &'static str,
    index: usize,
    paint: &PaintRow,
    window: &mut Window,
    cx: &mut Context<BoardView>,
) -> AnyElement {
    let this = cx.entity().downgrade();
    let id = format!("{key}-{index}");
    let row = row().items_center().min_h(px(28.));
    let row = match &paint.kind {
        PaintKind::Solid { hex, opacity } => {
            let of = ColourOf::Paint { key, index };
            let swatch = colour_swatch(view, &id, of, hex, *opacity, window, cx);
            let hex = TextField::new(format!("{key}.{index}.hex"), hex_rule(key, index))
                .render(view, sel, window, cx);
            let opacity =
                TextField::new(format!("{key}.{index}.opacity"), opacity_rule(key, index))
                    .width(OPACITY_WIDTH)
                    .render(view, sel, window, cx);
            row.child(swatch).child(hex).child(opacity)
        }
        other => {
            let (preview, name) = match other {
                PaintKind::Gradient(kind) => (gradient_swatch(cx), gradient_name(kind)),
                PaintKind::Image => (icon_swatch(IMAGE_ICON, cx), "Image".to_string()),
                _ => (icon_swatch(PATTERN_ICON, cx), "Pattern".to_string()),
            };
            let replace = move |sel: &DesignSelection| Some(replace_with_solid(sel, key, index));
            let replace = row_button(
                &id,
                "replace",
                REPLACE_ICON,
                "Replace with solid",
                &this,
                replace,
            );
            row.child(preview)
                .child(div().flex_1().min_w_0().truncate().child(name))
                .child(replace)
        }
    };
    let visible = paint.visible;
    let eye = eye_button(&id, visible, &this, move |sel| {
        Some(set_visible(sel, key, index, !visible))
    });
    let remove = row_button(&id, "remove", REMOVE_ICON, "Remove", &this, move |sel| {
        Some(remove_paint(sel, key, index))
    });
    row.child(eye).child(remove).into_any_element()
}

/// What a gradient row says it is: "Linear gradient".
fn gradient_name(kind: &str) -> String {
    if kind.eq_ignore_ascii_case("gradient") {
        "Gradient".to_string()
    } else {
        format!("{kind} gradient")
    }
}

/// A swatch of a gradient, for a row that holds one.
fn gradient_swatch(cx: &App) -> AnyElement {
    let theme = cx.theme();
    swatch_frame(cx)
        .bg(linear_gradient(
            90.,
            linear_color_stop(theme.background, 0.),
            linear_color_stop(theme.foreground, 1.),
        ))
        .into_any_element()
}

/// A swatch showing `icon`, for an image or a pattern row.
fn icon_swatch(icon: Lucide, cx: &App) -> AnyElement {
    swatch_frame(cx)
        .flex()
        .items_center()
        .justify_center()
        .text_color(cx.theme().muted_foreground)
        .child(Icon::new(icon).xsmall())
        .into_any_element()
}

/// The frame of a swatch that isn't a colour picker's.
fn swatch_frame(cx: &App) -> Div {
    div()
        .flex_shrink_0()
        .size(px(SWATCH))
        .rounded(px(4.))
        .border_1()
        .border_color(cx.theme().input)
        .overflow_hidden()
}

/// The hex and opacity of solid row `index` of list `key`, or `None` when it isn't a solid.
fn solid(sel: &DesignSelection, key: &str, index: usize) -> Option<(String, f64)> {
    match paints(sel, key)?.into_iter().nth(index)?.kind {
        PaintKind::Solid { hex, opacity } => Some((hex, opacity)),
        _ => None,
    }
}

/// The rule of the hex field of solid row `index` of list `key`: the colour without its `#`. A
/// typed colour (`RRGGBB` or `RGB`, the `#` left out or not; [`parse_hex`]) keeps the paint's
/// opacity, and anything else is refused.
fn hex_rule(key: &'static str, index: usize) -> Rule {
    Rule::new(
        move |sel| {
            solid(sel, key, index).map_or_else(SharedString::default, |(hex, _)| hex_text(&hex))
        },
        move |sel, typed| {
            let (_, opacity) = solid(sel, key, index)?;
            set_solid(sel, key, index, typed, opacity)
        },
    )
}

/// The rule of the opacity field of solid row `index` of list `key`: a percentage, which keeps the
/// paint's colour. A number typed without `%` is one (ruling P5-C7).
fn opacity_rule(key: &'static str, index: usize) -> Rule {
    Rule::new(
        move |sel| {
            let shown = solid(sel, key, index);
            shown.map_or_else(SharedString::default, |(_, opacity)| {
                number_field::percent(opacity)
            })
        },
        move |sel, typed| {
            let (hex, _) = solid(sel, key, index)?;
            let opacity = number_field::parse("opacity", typed)?;
            set_solid(sel, key, index, &hex, opacity)
        },
    )
}

/// A colour as a hex field shows it: `#D9D9D9` is "D9D9D9".
pub(super) fn hex_text(hex: &str) -> SharedString {
    hex.trim_start_matches('#').to_string().into()
}

/// The stroke position menu: inside, centre or outside the layers' edges, theirs checked.
fn stroke_align(sel: &DesignSelection, view: &WeakEntity<BoardView>) -> AnyElement {
    let current = match sel.get("strokeAlign") {
        Mixed::Same(Value::String(align)) => STROKE_ALIGNS
            .iter()
            .map(|(value, _)| *value)
            .find(|value| *value == align),
        _ => None,
    };
    let label = STROKE_ALIGNS
        .iter()
        .find(|(value, _)| Some(*value) == current)
        .map_or("Mixed", |(_, name)| *name);
    let choices = || {
        let named = STROKE_ALIGNS.iter();
        named
            .map(|(value, name)| (*value, (*name).into()))
            .collect()
    };
    let view = view.clone();
    let pick = move |align: &&'static str, cx: &mut App| {
        let align = *align;
        let _ = view.update(cx, |view, cx| {
            view.edit_layers(|sel| Some(sel.set("strokeAlign", json!(align))), cx);
        });
    };
    let choices = Choices::new(choices, current, pick);
    choice_menu("strokeAlign", label, "Stroke position", choices, false).into_any_element()
}

/// The dashed switch: on while every layer's stroke is dashed. Switched on, the strokes are
/// dashed 4 by 4; off, they are solid. A click on it gives it the keyboard, which goes back to the
/// canvas at once, so that the canvas's keys go on working.
fn dashed(sel: &DesignSelection, view: &WeakEntity<BoardView>) -> AnyElement {
    let on = matches!(sel.get("dashes"), Mixed::Same(Value::Array(dashes)) if !dashes.is_empty());
    let view = view.clone();
    let switch = Switch::new("design-dashes")
        .small()
        .checked(on)
        .label("Dashed")
        .on_click(move |dashed, window, cx| {
            let dashes = if *dashed { json!(DASHES) } else { json!([]) };
            let _ = view.update(cx, |view, cx| {
                view.edit_layers(|sel| Some(sel.set("dashes", dashes)), cx);
                window.focus(&view.focus, cx);
            });
        });
    // In a row, so that it is as wide as the switch and its label, and a click on it is theirs.
    row()
        .child(
            div()
                .debug_selector(|| "design-dashes".into())
                .child(switch),
        )
        .into_any_element()
}

/// What a colour picker colours.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ColourOf {
    /// Solid row `index` of list `key`, `fills` or `strokes`.
    Paint { key: &'static str, index: usize },
    /// The colour of shadow `index` of the effects.
    Shadow { index: usize },
}

impl ColourOf {
    /// The property a scrub of this colour changes.
    fn key(self) -> &'static str {
        match self {
            Self::Paint { key, .. } => key,
            Self::Shadow { .. } => "effects",
        }
    }

    /// The edit that gives this colour the hex `hex` and the opacity `opacity` on the layers of
    /// `sel`; `None` when they have no such solid paint or shadow.
    fn edit(self, sel: &DesignSelection, hex: &str, opacity: f64) -> Option<Edit> {
        match self {
            Self::Paint { key, index } => set_solid(sel, key, index, hex, opacity),
            Self::Shadow { index } => {
                let mut shadow = effects(sel)?.into_iter().nth(index)?;
                if !matches!(
                    shadow.kind,
                    EffectKind::DropShadow | EffectKind::InnerShadow
                ) {
                    return None;
                }
                shadow.hex = parse_hex(hex)?;
                shadow.opacity = opacity;
                Some(set_effect(sel, index, &shadow))
            }
        }
    }
}

/// A colour picker of the panel, and what it colours, kept from the first time it is drawn.
pub(super) struct Picker {
    pub(super) state: Entity<ColorPickerState>,
    /// What it colours, as it was last drawn.
    of: ColourOf,
    _events: [Subscription; 2],
}

/// The swatch of colour `of` of row `id`, `hex` at `opacity`, drawn as `design-<id>-swatch`: a
/// click opens the colour picker. While the picker is open, the colours it goes through are one
/// scrub of `of`'s list ([`BoardView::picker_shown`]); while it is closed, it shows the layers'
/// colour.
pub(super) fn colour_swatch(
    view: &mut BoardView,
    id: &str,
    of: ColourOf,
    hex: &str,
    opacity: f64,
    window: &mut Window,
    cx: &mut Context<BoardView>,
) -> AnyElement {
    let state = view.picker(id, of, window, cx);
    if let Some(colour) = colour_of(hex, opacity) {
        state.update(cx, |picker, cx| {
            if !picker.is_open() && picker.value() != Some(colour) {
                picker.set_value(colour, window, cx);
            }
        });
    }
    let selector = format!("design-{id}-swatch");
    div()
        .debug_selector(move || selector)
        .flex_shrink_0()
        .child(ColorPicker::new(&state).small())
        .into_any_element()
}

impl BoardView {
    /// Colour picker `id`, made the first time it is drawn, which colours `of`: the colours it
    /// picks go to the layers ([`Self::colour_picked`]), and its opening and closing start and end
    /// their scrub ([`Self::picker_shown`]).
    fn picker(
        &mut self,
        id: &str,
        of: ColourOf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<ColorPickerState> {
        if let Some(picker) = self.design.pickers.get_mut(id) {
            picker.of = of;
            return picker.state.clone();
        }
        let id = SharedString::from(id.to_string());
        let state = cx.new(|cx| ColorPickerState::new(window, cx));
        let picked = cx.subscribe_in(&state, window, {
            let id = id.clone();
            move |view, _, event: &ColorPickerEvent, _, cx| {
                if let ColorPickerEvent::Change(Some(colour)) = event {
                    view.colour_picked(&id, *colour, cx);
                }
            }
        });
        let shown = cx.observe_in(&state, window, {
            let id = id.clone();
            move |view, state, _, cx| {
                let open = state.read(cx).is_open();
                view.picker_shown(&id, open, cx);
            }
        });
        let picker = Picker {
            state: state.clone(),
            of,
            _events: [picked, shown],
        };
        self.design.pickers.insert(id, picker);
        state
    }

    /// Colour picker `id` is `open`, or closed. Opening it starts a scrub of the list it colours,
    /// so that every colour it goes through is one undo step (ruling P5-C15); closing it ends the
    /// scrub, kept. Escape ends it first, put back ([`Self::cancel_colour_scrub`]).
    pub(super) fn picker_shown(&mut self, id: &str, open: bool, cx: &mut Context<Self>) {
        let scrubbing = self.design.colour.as_deref() == Some(id);
        if open && !scrubbing {
            let Some(of) = self.design.pickers.get(id).map(|picker| picker.of) else {
                return;
            };
            self.scrub_begin(of.key(), cx);
            if self.scrub.is_some() {
                self.design.colour = Some(id.to_string().into());
            }
        } else if !open && scrubbing {
            self.scrub_end(true, cx);
        }
    }

    /// Colour picker `id` picked `colour`: what it colours takes it, on the selected layers, as
    /// they are now (within the picker's scrub while it is open).
    fn colour_picked(&mut self, id: &str, colour: Hsla, cx: &mut Context<Self>) {
        let Some(of) = self.design.pickers.get(id).map(|picker| picker.of) else {
            return;
        };
        let (hex, opacity) = hex_and_opacity(colour);
        self.edit_layers(|sel| of.edit(sel, &hex, opacity), cx);
    }

    /// Escape closes the colour picker that is open: the colours it went through go back, and no
    /// undo step is made (ruling P5-C15). Without a colour picker open it does nothing.
    pub(super) fn cancel_colour_scrub(&mut self, cx: &mut Context<Self>) {
        if self.design.colour.is_some() {
            self.scrub_end(false, cx);
        }
    }
}

/// The hex and opacity a paint takes for `colour`: `#RRGGBB`, and its alpha to a hundredth.
pub(super) fn hex_and_opacity(colour: Hsla) -> (String, f64) {
    let rgba = colour.to_rgb();
    let byte = |channel: f32| (channel.clamp(0.0, 1.0) * 255.0).round() as u8;
    let hex = format!(
        "#{:02X}{:02X}{:02X}",
        byte(rgba.r),
        byte(rgba.g),
        byte(rgba.b)
    );
    (hex, (f64::from(rgba.a) * 100.0).round() / 100.0)
}

/// The colour the picker holds for `hex` at `opacity`. `None` when `hex` is no colour.
pub(super) fn colour_of(hex: &str, opacity: f64) -> Option<Hsla> {
    let hex = parse_hex(hex)?;
    let channel = |at: usize| {
        let byte = u8::from_str_radix(hex.get(at..at + 2)?, 16).ok()?;
        Some(f32::from(byte) / 255.0)
    };
    let rgba = Rgba {
        r: channel(1)?,
        g: channel(3)?,
        b: channel(5)?,
        a: opacity.clamp(0.0, 1.0) as f32,
    };
    Some(rgba.into())
}

#[cfg(test)]
mod tests {
    use figcraft_doc::NodeId;
    use gpui_kit::component::color_picker::ColorPickerState;
    use gpui_kit::{Entity, TestAppContext, VisualTestContext};
    use serde_json::{Value, json};

    // Not `super::*`: gpui-kit's `test` attribute would come with it, in place of the standard one.
    use super::super::tests::{
        Design, Scratch, click, drawn, field_text, focus_canvas, open_in_window, prop, rectangle,
        undo_steps,
    };
    use super::{colour_of, gradient_name, hex_and_opacity, hex_text};
    use crate::board::BoardView;

    /// Writes a design file at `path` with a rectangle at each of `xs` whose fills are `fills`
    /// (as `node.set` takes them), and returns their ids.
    fn with_fills(path: &std::path::Path, xs: &[f64], fills: &[Value]) -> Vec<NodeId> {
        let mut design = Design::new();
        let ids: Vec<NodeId> = xs.iter().map(|x| design.make(rectangle(*x))).collect();
        for (id, fills) in ids.iter().zip(fills) {
            design.set(&[*id], json!({ "fills": fills }));
        }
        design.save(path);
        ids
    }

    /// Colour picker `id`'s state.
    fn picker(
        view: &Entity<BoardView>,
        id: &str,
        cx: &VisualTestContext,
    ) -> Entity<ColorPickerState> {
        view.read_with(cx, |view, _| view.design.pickers[id].state.clone())
    }

    /// Has picker `picker` pick `hex`, as a drag of its sliders does: it stays open.
    fn pick(picker: &Entity<ColorPickerState>, hex: &str, cx: &mut VisualTestContext) {
        let colour = colour_of(hex, 1.0).unwrap();
        cx.update(|window, cx| {
            picker.update(cx, |picker, cx| picker.update_color(colour, window, cx));
        });
        cx.run_until_parked();
    }

    /// Types `text` over what field `selector` shows, and presses Enter.
    fn type_in(cx: &mut VisualTestContext, selector: &'static str, text: &str) {
        click(cx, selector);
        // A field that has the keyboard already keeps its caret where the click put it.
        cx.simulate_keystrokes("cmd-a");
        cx.simulate_input(text);
        cx.simulate_keystrokes("enter");
    }

    /// Whether picker `picker` is open.
    fn is_open(picker: &Entity<ColorPickerState>, cx: &VisualTestContext) -> bool {
        picker.read_with(cx, |picker, _| picker.is_open())
    }

    /// The tool the canvas has on.
    fn tool(view: &Entity<BoardView>, cx: &VisualTestContext) -> &'static str {
        view.read_with(cx, |view, _| view.board().unwrap().session.tool.tool.id())
    }

    /// Ruling P5-C15: the colours a picker goes through while it is open are one undo step.
    #[gpui_kit::test]
    fn color_pick_is_one_undo_step(cx: &mut TestAppContext) {
        let tmp = Scratch::new("colour-pick");
        let path = tmp.0.join("design.figcraft");
        let ids = with_fills(&path, &[0.0], &[json!(["#D9D9D9"])]);
        let (view, mut cx) = open_in_window(cx, &path, &ids);
        let colour = |cx: &VisualTestContext| prop(&view, ids[0], "fills", cx)[0]["color"].clone();
        assert_eq!(colour(&cx), json!("#D9D9D9"));
        let steps = undo_steps(&view, &cx);
        // The swatch opens the picker, and with it the scrub.
        click(&mut cx, "design-fills-0-swatch");
        let picker = picker(&view, "fills-0", &cx);
        assert!(is_open(&picker, &cx));
        assert!(view.read_with(&cx, |view, _| view.scrub.is_some()));
        for hex in ["#FF0000", "#00FF00", "#0000FF"] {
            pick(&picker, hex, &mut cx);
            // The canvas shows each colour as it comes, with no undo step of its own.
            assert_eq!(colour(&cx), json!(hex));
            assert_eq!(undo_steps(&view, &cx), steps);
        }
        // Closing the picker ends the scrub, as one undo step.
        click(&mut cx, "design-fills-0-swatch");
        assert!(!is_open(&picker, &cx));
        assert!(view.read_with(&cx, |view, _| view.scrub.is_none()));
        assert_eq!(colour(&cx), json!("#0000FF"));
        assert_eq!(undo_steps(&view, &cx), steps + 1);
        view.update(&mut cx, |view, cx| view.choose("edit.undo", json!({}), cx));
        assert_eq!(colour(&cx), json!("#D9D9D9"));
        assert_eq!(view.read_with(&cx, |view, _| view.toast.clone()), None);
    }

    /// Ruling P5-C15: Escape closes the picker and puts the colour back, with no undo step.
    #[gpui_kit::test]
    fn escape_puts_a_colour_pick_back(cx: &mut TestAppContext) {
        let tmp = Scratch::new("colour-escape");
        let path = tmp.0.join("design.figcraft");
        let ids = with_fills(&path, &[0.0], &[json!(["#D9D9D9"])]);
        let (view, mut cx) = open_in_window(cx, &path, &ids);
        let colour = |cx: &VisualTestContext| prop(&view, ids[0], "fills", cx)[0]["color"].clone();
        let steps = undo_steps(&view, &cx);
        click(&mut cx, "design-fills-0-swatch");
        let picker = picker(&view, "fills-0", &cx);
        pick(&picker, "#FF0000", &mut cx);
        assert_eq!(colour(&cx), json!("#FF0000"));
        cx.simulate_keystrokes("escape");
        assert!(!is_open(&picker, &cx));
        assert!(view.read_with(&cx, |view, _| view.scrub.is_none()));
        assert_eq!(colour(&cx), json!("#D9D9D9"));
        assert_eq!(undo_steps(&view, &cx), steps);
        // The swatch shows the colour that stayed.
        let shown = picker.read_with(&cx, |picker, _| picker.value());
        assert_eq!(shown, colour_of("#D9D9D9", 1.0));
    }

    /// Review Focus 1 and 2 for a paint: a typed hex or opacity goes to the paint of every
    /// selected layer, as one undo step, and what isn't one is refused, the field showing the
    /// paint's again.
    #[gpui_kit::test]
    fn a_paints_hex_and_opacity_are_typed(cx: &mut TestAppContext) {
        let tmp = Scratch::new("paint-fields");
        let path = tmp.0.join("design.figcraft");
        let fills = json!(["#D9D9D9"]);
        let ids = with_fills(&path, &[0.0, 150.0], &[fills.clone(), fills]);
        let (view, mut cx) = open_in_window(cx, &path, &ids);
        assert_eq!(field_text(&view, "fills.0.hex", &cx), "D9D9D9");
        assert_eq!(field_text(&view, "fills.0.opacity", &cx), "100%");
        let steps = undo_steps(&view, &cx);
        type_in(&mut cx, "design-field-fills.0.hex", "ff8800");
        for id in &ids {
            assert_eq!(prop(&view, *id, "fills", &cx)[0]["color"], json!("#FF8800"));
        }
        assert_eq!(undo_steps(&view, &cx), steps + 1);
        assert_eq!(field_text(&view, "fills.0.hex", &cx), "FF8800");
        // No colour: nothing changes, and the field shows the paint's again.
        type_in(&mut cx, "design-field-fills.0.hex", "zz");
        assert_eq!(field_text(&view, "fills.0.hex", &cx), "FF8800");
        assert_eq!(undo_steps(&view, &cx), steps + 1);
        assert_eq!(view.read_with(&cx, |view, _| view.toast.clone()), None);
        // 50 in the opacity field is half (ruling P5-C7), and keeps the colour.
        type_in(&mut cx, "design-field-fills.0.opacity", "50");
        for id in &ids {
            let fill = &prop(&view, *id, "fills", &cx)[0];
            assert_eq!(
                (&fill["color"], &fill["opacity"]),
                (&json!("#FF8800"), &json!(0.5))
            );
        }
        assert_eq!(field_text(&view, "fills.0.opacity", &cx), "50%");
    }

    /// Review Focus 4 through the panel: a gradient beside an edited solid stays as it is, and
    /// becomes a solid only by its own button.
    #[gpui_kit::test]
    fn a_gradient_becomes_a_solid_only_when_asked(cx: &mut TestAppContext) {
        let tmp = Scratch::new("paint-gradient");
        let path = tmp.0.join("design.figcraft");
        let gradient = json!({"type": "linearGradient", "stops": ["#FF0000", "#0000FF"]});
        let ids = with_fills(&path, &[0.0], &[json!([gradient, "#112233"])]);
        let (view, mut cx) = open_in_window(cx, &path, &ids);
        let fills = |cx: &VisualTestContext| prop(&view, ids[0], "fills", cx);
        let before = fills(&cx)[0].clone();
        assert!(!drawn(&mut cx, "design-fills-0-swatch"));
        assert!(drawn(&mut cx, "design-fills-1-swatch"));
        type_in(&mut cx, "design-field-fills.1.hex", "ff8800");
        assert_eq!(fills(&cx)[0], before);
        assert_eq!(fills(&cx)[1]["color"], json!("#FF8800"));
        click(&mut cx, "design-fills-0-replace");
        assert_eq!(fills(&cx)[0]["type"], json!("solid"));
        assert_eq!(fills(&cx)[0]["color"], json!("#FF0000"));
        assert!(drawn(&mut cx, "design-fills-0-swatch"));
    }

    /// Paints that differ show "Mixed"; Replace all gives every layer one paint, as one undo step;
    /// then the eye hides it and the remove button takes it off.
    #[gpui_kit::test]
    fn mixed_fills_are_replaced_all_at_once(cx: &mut TestAppContext) {
        let tmp = Scratch::new("paint-mixed");
        let path = tmp.0.join("design.figcraft");
        let ids = with_fills(
            &path,
            &[0.0, 150.0],
            &[json!(["#112233"]), json!(["#445566"])],
        );
        let (view, mut cx) = open_in_window(cx, &path, &ids);
        let fills = |id: NodeId, cx: &VisualTestContext| prop(&view, id, "fills", cx);
        assert!(drawn(&mut cx, "design-fills-replace-all"));
        assert!(!drawn(&mut cx, "design-fills-0-swatch"));
        let steps = undo_steps(&view, &cx);
        click(&mut cx, "design-fills-replace-all");
        for id in &ids {
            let fills = fills(*id, &cx);
            assert_eq!(fills.as_array().map(Vec::len), Some(1), "{fills}");
            assert_eq!(fills[0]["color"], json!("#D9D9D9"));
        }
        assert_eq!(undo_steps(&view, &cx), steps + 1);
        click(&mut cx, "design-fills-0-visible");
        for id in &ids {
            assert_eq!(fills(*id, &cx)[0]["visible"], json!(false));
        }
        click(&mut cx, "design-fills-0-remove");
        for id in &ids {
            assert_eq!(fills(*id, &cx), Value::Null, "no fill left");
        }
    }

    /// The Stroke section: a stroke added shows its weight, position and dashes. Neither a pick
    /// from its menu nor a click on its switch keeps the keyboard from the canvas (delegate
    /// ruling on P5.T4): the canvas's keys work after them with no click on it.
    #[gpui_kit::test]
    fn the_stroke_section_edits_the_strokes(cx: &mut TestAppContext) {
        let tmp = Scratch::new("strokes");
        let path = tmp.0.join("design.figcraft");
        let ids = with_fills(&path, &[0.0], &[json!(["#D9D9D9"])]);
        let (view, mut cx) = open_in_window(cx, &path, &ids);
        let value = |key: &str, cx: &VisualTestContext| prop(&view, ids[0], key, cx);
        assert!(!drawn(&mut cx, "design-field-strokeWeight"));
        click(&mut cx, "design-strokes-add");
        assert_eq!(value("strokes", &cx)[0]["color"], json!("#000000"));
        type_in(&mut cx, "design-field-strokeWeight", "4");
        assert_eq!(value("strokeWeight", &cx), json!(4.0));

        focus_canvas(&view, &mut cx);
        click(&mut cx, "design-strokeAlign");
        click(&mut cx, "design-strokeAlign-Outside");
        assert_eq!(value("strokeAlign", &cx), json!("outside"));
        cx.simulate_keystrokes("r");
        assert_eq!(tool(&view, &cx), "rectangle");

        click(&mut cx, "design-dashes");
        assert_eq!(value("dashes", &cx), json!([4.0, 4.0]));
        cx.simulate_keystrokes("o");
        assert_eq!(tool(&view, &cx), "ellipse");
        click(&mut cx, "design-dashes");
        assert_eq!(value("dashes", &cx), Value::Null, "solid again");
        assert_eq!(view.read_with(&cx, |view, _| view.toast.clone()), None);
    }

    #[test]
    fn a_colour_goes_through_the_picker_and_back() {
        for (hex, opacity) in [
            ("#D9D9D9", 1.0),
            ("#000000", 0.25),
            ("#FF8800", 0.5),
            ("#0D99FF", 0.0),
        ] {
            let colour = colour_of(hex, opacity).unwrap();
            assert_eq!(hex_and_opacity(colour), (hex.to_string(), opacity), "{hex}");
        }
        // Any form of hex the field takes; anything else is no colour.
        assert!(colour_of("d9d9d9", 1.0).is_some());
        assert!(colour_of("#12", 1.0).is_none());
    }

    #[test]
    fn a_hex_shows_without_its_hash() {
        assert_eq!(hex_text("#D9D9D9").as_ref(), "D9D9D9");
    }

    #[test]
    fn a_gradient_is_named_by_its_kind() {
        assert_eq!(gradient_name("Linear"), "Linear gradient");
        assert_eq!(gradient_name("Gradient"), "Gradient");
    }
}
