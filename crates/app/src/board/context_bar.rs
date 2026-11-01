//! The context bar (spec, "Toolbar and context bar"; FigCraft's `ui-egui/src/canvas/board_bar.rs`
//! ported): the row of small controls floating over a board's selection with what its sticky
//! notes, shapes, connectors and ink share (`sawit_board::context_bar`). The colours are swatches;
//! the shape, the text size, how connectors run and their caps are menus. Every control shows the
//! options the model lists for it, and a pick runs its option's command through
//! [`BoardView::choose`], but for the shape the selection has already: the engine refuses that
//! one, so picking it does nothing (ruling P3-C2).
//!
//! Each menu's button shows the picture of the selection's choice ([`super::option_icons`]), and
//! each item its picture beside its words. The shape and the text size show their words on the
//! button too; how connectors run and their two caps are the picture alone, as FigJam's bar draws
//! them, with their words in the tooltip and the name VoiceOver reads ("Line: Elbow", "Start:
//! None", "End: Triangle arrow"). The start cap's pictures face left and the end cap's right, so
//! the two buttons read as the two ends of the line.
//!
//! The bar's bottom goes 12 px above the selection, and the bar stays inside the canvas
//! (`context_bar::clamp_anchor`, ruling P3-C3), over the toolbar that floats at the canvas's
//! bottom: under it, the toolbar would cover the bar. It is never wider than the canvas less 8 px
//! at each side: in a narrower canvas its controls keep their size and scroll sideways inside it,
//! as the toolbar's buttons do. It hides while a gesture is on (a drag) and while a text is edited,
//! and with the rest of the chrome while the UI is hidden.

use figcraft_engine::guard;
use gpui_kit::component::button::*;
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenu, PopupMenuItem};
use gpui_kit::component::*;
use gpui_kit::*;
use sawit_board::context_bar::{self as model, Control, ControlOption};

use super::BoardView;
use super::option_icons::{self, OptionIcon};
use super::toolbar::{self, SWATCH, divider, swatch};

/// The bar's height.
pub(super) const HEIGHT: f64 = 40.;
/// The room between the bar's edge and its controls. Half of it is the bar's padding and half the
/// padding of the strip its controls scroll in ([`render_context_bar`]).
const PADDING: f64 = 6.;
/// The width of the bar's border.
const BORDER: f64 = 1.;
/// The gap between two swatches.
const GAP: f64 = 2.;
/// The room a line between two controls takes: the line and a margin each side
/// ([`divider`]).
const DIVIDER: f64 = 9.;

/// The side of a choice's picture on a button that shows the picture alone.
const GLYPH: f32 = 16.;
/// The side of a choice's picture beside words, on a button that shows its words too.
const SMALL_GLYPH: f32 = option_icons::MENU_GLYPH;

/// What the bar shows when the selected items' choices differ.
const MIXED: &str = "Mixed";

/// How wide `control` is: its swatches side by side, or its menu's button. A button with words
/// fits the longest it can show, "Mixed" included, in 14 px text (a small button's text at a 16 px
/// rem; Sawit's 13 px rem draws it at 11.375 px), plus 34 px for its padding, the gap and the
/// caret, and the picture and its gap. A longer label is cut short with an ellipsis. A button of a
/// picture alone (how connectors run, their caps) is the picture, its padding and the caret, or as
/// wide as "Mixed" when the items differ.
fn width(control: &Control) -> f64 {
    let picture = f64::from(SMALL_GLYPH) + 4.0;
    match control {
        Control::Colors { items, .. } => {
            let swatches = items.len() as f64;
            swatches * f64::from(SWATCH) + (swatches - 1.0).max(0.0) * GAP
        }
        // "Rounded rectangle", "Speech bubble".
        Control::Shape { .. } => 160. + picture,
        // "Mixed", which it shows when the selected texts' sizes differ or aren't S, M, L or XL:
        // 38.5 px in 14 px text, over twice the longest size, "XL", and its picture.
        Control::FontSize { .. } => 76.,
        Control::Routing { current: None }
        | Control::StartCap { current: None }
        | Control::EndCap { current: None } => 68.,
        Control::Routing { .. } | Control::StartCap { .. } | Control::EndCap { .. } => 48.,
    }
}

/// The size of the bar that holds `controls` with room for all of them: their widths, the lines
/// between them, its padding and border. A canvas too narrow for it gets a narrower bar
/// ([`render_context_bar`]).
pub(super) fn size(controls: &[Control]) -> (f64, f64) {
    let widths: f64 = controls.iter().map(width).sum();
    let lines = controls.len().saturating_sub(1) as f64 * DIVIDER;
    (widths + lines + 2.0 * (PADDING + BORDER), HEIGHT)
}

/// What the tooltip of `control`'s menu calls it, before the choice it shows ("Start: None").
fn name(control: &Control) -> &'static str {
    match control {
        Control::Colors { .. } => "Color",
        Control::Shape { .. } => "Shape",
        Control::FontSize { .. } => "Text size",
        Control::Routing { .. } => "Line",
        Control::StartCap { .. } => "Start",
        Control::EndCap { .. } => "End",
    }
}

/// The heading of `control`'s menu, for the two cap menus, whose items are alike but for the way
/// their pictures face.
fn heading(control: &Control) -> Option<&'static str> {
    match control {
        Control::StartCap { .. } => Some("Line start"),
        Control::EndCap { .. } => Some("Line end"),
        _ => None,
    }
}

/// Whether `control`'s button shows its choice's picture alone, its words going to its tooltip.
fn picture_alone(control: &Control) -> bool {
    matches!(
        control,
        Control::Routing { .. } | Control::StartCap { .. } | Control::EndCap { .. }
    )
}

/// The context bar over the selection of `view`'s board, if it has one to show: not while a
/// gesture is on or a text is edited, nor when nothing selected takes a control. The model is read
/// on every frame, behind the engine's guard: a panic in it costs that frame its bar (D15).
///
/// The bar takes the pointer: no click or scroll over it reaches the canvas under it. It keeps its
/// full width ([`size`]) while the canvas has room for it and [`model::MARGIN`] at each side, and
/// is never wider than the canvas less that room, so that the clamp keeps all of it inside the
/// canvas (spec, "The bar is clamped inside the canvas"). Narrower than its controls, it keeps them
/// at their size and they scroll sideways inside it, under a swipe or the wheel: the toolbar's rule
/// (`toolbar::render_toolbar`).
pub(super) fn render_context_bar(
    view: &mut BoardView,
    _: &mut Window,
    cx: &mut Context<BoardView>,
) -> Option<impl IntoElement + use<>> {
    if view.editing_text() {
        return None;
    }
    let board = view.board()?;
    let session = &board.session;
    if session.tool.busy() {
        return None;
    }
    let ds = session.ds().ok()?;
    // The room the bar is kept in: the canvas above the toolbar, which shows whenever the bar
    // does. FigCraft's own bar keeps clear of its toolbar too (ui-egui `board_bar.rs`, `place`).
    let under = f64::from(toolbar::BOTTOM + toolbar::HEIGHT);
    let canvas = (ds.view.vw, ds.view.vh - under);
    let (bar, options) = guard::run("ui.contextBar", || {
        Ok(model::context_bar(session).map(|bar| {
            let options: Vec<Vec<ControlOption>> =
                bar.controls.iter().map(model::options).collect();
            (bar, options)
        }))
    })
    .ok()
    .flatten()?;
    let (full, height) = size(&bar.controls);
    // Never wider than the canvas less its margins, which the clamp then keeps the bar inside.
    let width = full.min(canvas.0 - 2.0 * model::MARGIN).max(0.0);
    // The anchor is where the bar's bottom-centre goes; the clamp takes and gives its top-centre.
    let top_centre = (bar.anchor.0, bar.anchor.1 - height);
    let (x, y) = model::clamp_anchor(top_centre, (width, height), canvas);
    let this = cx.entity().downgrade();
    let theme = cx.theme();
    let line = theme.border;
    // The controls and the lines between them, at their full size: the row never shrinks.
    let mut row = h_flex().flex_shrink_0();
    for (i, (control, options)) in bar.controls.iter().zip(options).enumerate() {
        if i > 0 {
            row = row.child(divider(line));
        }
        row = match control {
            Control::Colors { .. } => row.child(swatches(options, &this)),
            _ => row.child(menu(control, options, &this)),
        };
    }
    // The part of the row the bar has room for, as in the toolbar: a swipe scrolls it sideways,
    // and so does the wheel, which GPUI turns sideways in a box that only scrolls sideways. The
    // row is cut inside the bar's border, and the strip's half of the padding keeps a button's
    // focus ring, drawn outside it, off the cut. The menus still open whole: GPUI draws their
    // popovers last, uncut.
    let inset = px((PADDING / 2.0) as f32);
    let strip = h_flex()
        .id("board-context-bar-strip")
        .flex_1()
        .min_w_0()
        .h_full()
        .px(inset)
        .overflow_x_scroll()
        .child(row);
    Some(
        h_flex()
            .id("board-context-bar")
            .debug_selector(|| "board-context-bar".into())
            // Its clicks and scrolls are its own: none reaches the canvas under it.
            .occlude()
            .absolute()
            .left(px((x - width / 2.0) as f32))
            .top(px(y as f32))
            .w(px(width as f32))
            .h(px(height as f32))
            .px(inset)
            .rounded(px(10.))
            .border_1()
            .border_color(theme.border)
            .bg(theme.popover)
            .text_color(theme.popover_foreground)
            .shadow_md()
            .child(strip),
    )
}

/// The board colours as swatches, the selection's own marked.
fn swatches(options: Vec<ControlOption>, view: &WeakEntity<BoardView>) -> Div {
    let swatches = options.into_iter().enumerate().map(|(i, option)| {
        let (command, params, view) = (option.command, option.params, view.clone());
        let rgba = option.swatch.unwrap_or_default();
        let pick = move |_: &ClickEvent, _: &mut Window, cx: &mut App| {
            let _ = view.update(cx, |view, cx| view.choose(command, params.clone(), cx));
        };
        let name = option.label.clone();
        swatch(
            ("board-context-color", i),
            rgba,
            &name,
            option.selected,
            pick,
        )
        .debug_selector(move || format!("board-context-color-{name}"))
    });
    h_flex().gap(px(GAP as f32)).children(swatches)
}

/// The picture and the words of the choice a menu's button shows: `picture` at `side` px, drawn
/// as `<id>-picture`, then `words` when given.
fn face(id: &'static str, picture: Option<OptionIcon>, side: f32, words: Option<String>) -> Div {
    h_flex()
        .min_w_0()
        .gap_1()
        .children(picture.map(|picture| {
            div()
                .flex_none()
                .debug_selector(move || format!("{id}-picture"))
                .child(picture.element(px(side)))
        }))
        .children(words.map(|words| {
            div()
                .min_w_0()
                .whitespace_nowrap()
                .text_ellipsis()
                .child(words)
        }))
}

/// The menu of `control`: its button shows the picture of the selection's choice, with its words
/// unless [`picture_alone`], or "Mixed" when the items differ, and is named "Shape: Rectangle" in
/// its tooltip and to VoiceOver. Its items are `options`, each with its picture before its words
/// and the selection's checked.
fn menu(
    control: &Control,
    options: Vec<ControlOption>,
    view: &WeakEntity<BoardView>,
) -> impl IntoElement + use<> {
    let id = match control {
        Control::Colors { .. } => "board-context-colors",
        Control::Shape { .. } => "board-context-shape",
        Control::FontSize { .. } => "board-context-size",
        Control::Routing { .. } => "board-context-routing",
        Control::StartCap { .. } => "board-context-start",
        Control::EndCap { .. } => "board-context-end",
    };
    let current = options.iter().find(|option| option.selected);
    let named = format!(
        "{}: {}",
        name(control),
        current.map_or(MIXED, |option| option.label.as_str())
    );
    let shown = match current {
        Some(option) if picture_alone(control) => {
            face(id, option_icons::of_params(&option.params), GLYPH, None)
        }
        Some(option) => face(
            id,
            option_icons::of_params(&option.params),
            SMALL_GLYPH,
            Some(option.label.clone()),
        ),
        None => face(id, None, SMALL_GLYPH, Some(MIXED.to_owned())),
    };
    // The engine refuses the shape the selection has already: picking it does nothing.
    let shape = matches!(control, Control::Shape { .. });
    let heading = heading(control);
    let view = view.clone();
    Button::new(id)
        .ghost()
        .small()
        .child(shown)
        .dropdown_caret(true)
        .w(px(width(control) as f32))
        .accessibility_label(named.clone())
        .tooltip(named)
        .debug_selector(move || id.to_owned())
        .dropdown_menu(move |mut menu: PopupMenu, _, _| {
            if let Some(heading) = heading {
                menu = menu.label(heading);
            }
            for option in &options {
                let runs = !(shape && option.selected);
                let (command, params, view) = (option.command, option.params.clone(), view.clone());
                let label = option.label.clone();
                let picture = option_icons::of_params(&params);
                let item = PopupMenuItem::element(move |_, _| {
                    let label = label.clone();
                    // The words as text a screen reader reads: a menu item drawn from elements
                    // has no name of its own.
                    let words = crate::catalog::label(
                        SharedString::from(format!("{id}-{label}")),
                        label.clone(),
                    );
                    option_icons::menu_row(picture, words)
                        .debug_selector(move || format!("{id}-{label}"))
                })
                .checked(option.selected)
                .on_click(move |_, _, cx| {
                    if runs {
                        let _ =
                            view.update(cx, |view, cx| view.choose(command, params.clone(), cx));
                    }
                });
                menu = menu.item(item);
            }
            menu
        })
}

#[cfg(test)]
mod tests {
    use gpui_kit::{TestAppContext, VisualTestContext, px};
    use sawit_board::context_bar::{Control, context_bar};
    use serde_json::json;

    use super::super::tests::{Scratch, chrome_canvas, click, run_op};
    use super::{GLYPH, width};

    /// The value the context bar's end cap control shows for the selection.
    fn end_cap(
        view: &gpui_kit::Entity<super::BoardView>,
        cx: &VisualTestContext,
    ) -> Option<String> {
        view.read_with(cx, |view, _| {
            let bar = context_bar(&view.board().unwrap().session)?;
            bar.controls.into_iter().find_map(|control| match control {
                Control::EndCap { current } => current,
                _ => None,
            })
        })
    }

    /// The bug the user saw: three text dropdowns, nothing saying which end each cap is. Each now
    /// shows its choice's picture, at a size of its own, the caps' facing their ends, and a pick
    /// from the end cap's menu sets the end cap.
    #[gpui_kit::test]
    fn the_connector_menus_show_their_choices_pictures(cx: &mut TestAppContext) {
        let tmp = Scratch::new("context-pictures");
        let (view, mut cx) = chrome_canvas(cx, &tmp);
        let connector = json!({
            "op": "command", "command": "board.connector",
            "params": {"from": {"x": -200, "y": 100}, "to": {"x": 200, "y": 100}},
        });
        run_op(&view, connector, &mut cx);
        cx.run_until_parked();
        // The picture alone, and the caret: no words.
        let alone = width(&Control::EndCap {
            current: Some("none".into()),
        });
        for (id, picture) in [
            ("board-context-routing", "board-context-routing-picture"),
            ("board-context-start", "board-context-start-picture"),
            ("board-context-end", "board-context-end-picture"),
        ] {
            let button = cx
                .debug_bounds(id)
                .unwrap_or_else(|| panic!("{id} isn't drawn"));
            assert_eq!(f64::from(button.size.width), alone, "{id}");
            let drawn = cx
                .debug_bounds(picture)
                .unwrap_or_else(|| panic!("{picture}"));
            assert_eq!(drawn.size.width, px(GLYPH), "{id}");
            assert!(button.contains(&drawn.center()), "{id}");
        }

        // The end cap's menu lists the caps with their pictures; Circle sets it.
        click(&mut cx, "board-context-end");
        cx.run_until_parked();
        click(&mut cx, "board-context-end-Circle");
        cx.run_until_parked();
        assert_eq!(end_cap(&view, &cx).as_deref(), Some("circleFilled"));
        assert!(cx.debug_bounds("board-context-end-picture").is_some());
    }

    /// Connectors whose caps differ show "Mixed", in words, on a button wide enough for them, and
    /// no picture.
    #[gpui_kit::test]
    fn a_mixed_choice_shows_its_words(cx: &mut TestAppContext) {
        let tmp = Scratch::new("context-mixed");
        let (view, mut cx) = chrome_canvas(cx, &tmp);
        let mut ids = Vec::new();
        for (y, cap) in [(0, "none"), (120, "triangleArrow")] {
            let connector = json!({
                "op": "command", "command": "board.connector",
                "params": {"from": {"x": -200, "y": y}, "to": {"x": 200, "y": y}},
            });
            // The new connector is selected, and the style goes to it.
            run_op(&view, connector, &mut cx);
            let style = json!({
                "op": "command", "command": "board.setStyle", "params": {"endCap": cap},
            });
            run_op(&view, style, &mut cx);
            let id = view.read_with(&cx, |view, _| {
                let ds = view.board().unwrap().session.ds().unwrap();
                ds.selection.first().copied().unwrap()
            });
            ids.push(id);
        }
        let select = json!({"op": "command", "command": "select.set", "params": {"ids": ids}});
        run_op(&view, select, &mut cx);
        cx.run_until_parked();
        assert_eq!(end_cap(&view, &cx), None);
        let button = cx.debug_bounds("board-context-end").expect("the end cap");
        let mixed = width(&Control::EndCap { current: None });
        assert_eq!(f64::from(button.size.width), mixed);
        assert!(cx.debug_bounds("board-context-end-picture").is_none());
    }
}
