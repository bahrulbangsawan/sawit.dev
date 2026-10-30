//! The Typography section (spec "Design panel" 8), shown while a text is selected: the font family,
//! picked from a list that can be searched; the style; the size, line height and letter spacing;
//! the horizontal and vertical alignment; and how the text box resizes. An edit goes to the
//! selected texts only, and restyles the whole of each (`sawit_board::design::DesignSelection::set`).
//!
//! - The family list is every family the fonts have (`sawit_board::design::typography::families`),
//!   listed again when the fonts change: the system's fonts come in from a scan that may end after
//!   the panel is first drawn (spec D13). A family a text names that the fonts don't have shows in
//!   the list's button all the same. The list takes the keyboard while it is open, for its search,
//!   and gives it back to the canvas once it closes, on a pick or on Escape (delegate ruling on
//!   P5.T4).
//! - The line height takes `auto`, a number of pixels or a percentage, and the letter spacing a
//!   number of pixels or a percentage (ruling P5-C8). A line height can't be below 0.

use std::rc::Rc;

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::searchable_list::SearchableVec;
use gpui_kit::component::select::{Select, SelectEvent, SelectState};
use gpui_kit::component::*;
use gpui_kit::*;
use sawit_board::design::typography::{families, styles};
use sawit_board::design::{DesignSelection, Mixed};
use serde_json::{Value, json};

use super::number_field::{NumberField, Rule, TextField};
use super::{Choices, choice_menu, guarded, icon_button, on_click_edit, row, section};
use crate::board::BoardView;

/// The font size field's picture.
pub(super) const SIZE_ICON: Lucide = Lucide::ALargeSmall;
/// The line height field's picture, and the letter spacing field's.
pub(super) const LINE_HEIGHT_ICON: Lucide = Lucide::UnfoldVertical;
pub(super) const LETTER_SPACING_ICON: Lucide = Lucide::UnfoldHorizontal;

/// A row of buttons that set one property of the texts: each one's value, picture and tooltip.
type Buttons = [(&'static str, Lucide, &'static str)];

/// The horizontal alignments.
const ALIGNS: [(&str, Lucide, &str); 4] = [
    ("left", Lucide::TextAlignStart, "Align left"),
    ("center", Lucide::TextAlignCenter, "Align center"),
    ("right", Lucide::TextAlignEnd, "Align right"),
    ("justified", Lucide::TextAlignJustify, "Justify"),
];

/// The vertical alignments.
const VERTICAL_ALIGNS: [(&str, Lucide, &str); 3] = [
    ("top", Lucide::ArrowUpToLine, "Align top"),
    ("middle", Lucide::FoldVertical, "Align middle"),
    ("bottom", Lucide::ArrowDownToLine, "Align bottom"),
];

/// How the text box resizes, as FigCraft's own panel offers it.
const RESIZES: [(&str, Lucide, &str); 4] = [
    ("widthAndHeight", Lucide::MoveHorizontal, "Auto width"),
    ("height", Lucide::MoveVertical, "Auto height"),
    ("none", Lucide::Square, "Fixed size"),
    ("truncate", Lucide::Ellipsis, "Truncate text"),
];

/// The font families the list offers.
type FamilyItems = SearchableVec<SharedString>;

/// The font family list, kept from the first time it is drawn.
pub(super) struct Families {
    state: Entity<SelectState<FamilyItems>>,
    /// The fonts it lists: `figcraft_text::layout_generation` when it listed them.
    generation: u64,
    /// The family it was last set to show, `None` when the texts' families differ.
    shown: Option<SharedString>,
    _events: [Subscription; 2],
}

/// Every icon the section shows, for the test that they are all embedded (ruling P5-C11).
#[cfg(test)]
pub(super) fn icons() -> impl Iterator<Item = Lucide> {
    let rows = ALIGNS.iter().chain(&VERTICAL_ALIGNS).chain(&RESIZES);
    let icons = rows.map(|(_, icon, _)| *icon);
    icons.chain([SIZE_ICON, LINE_HEIGHT_ICON, LETTER_SPACING_ICON])
}

/// The Typography section for the texts among the layers of `sel`.
pub(super) fn render(
    view: &mut BoardView,
    sel: &Rc<DesignSelection>,
    window: &mut Window,
    cx: &mut Context<BoardView>,
) -> AnyElement {
    let this = cx.entity().downgrade();
    let family = family_button(view, sel, window, cx);
    let style = style_menu(sel, &this);
    let size = NumberField::new("fontSize", "Font size")
        .icon(SIZE_ICON)
        .render(view, sel, window, cx);
    let line_height = TextField::new("lineHeight", spacing_rule("lineHeight", parse_line_height))
        .label("Line height")
        .icon(LINE_HEIGHT_ICON)
        .render(view, sel, window, cx);
    let letter_spacing = TextField::new(
        "letterSpacing",
        spacing_rule("letterSpacing", parse_letter_spacing),
    )
    .label("Letter spacing")
    .icon(LETTER_SPACING_ICON)
    .render(view, sel, window, cx);
    let aligns = h_flex()
        .w_full()
        .justify_between()
        .child(buttons("textAlign", &ALIGNS, sel, &this))
        .child(buttons("textAlignVertical", &VERTICAL_ALIGNS, sel, &this));
    let rows = vec![
        family,
        row().child(style).child(size).into_any_element(),
        row()
            .child(line_height)
            .child(letter_spacing)
            .into_any_element(),
        aligns.into_any_element(),
        buttons("textAutoResize", &RESIZES, sel, &this).into_any_element(),
    ];
    section("typography", "Typography", Vec::new(), rows, cx)
}

/// The font family list's button, drawn as `design-fontFamily`: the texts' family, or "Mixed".
fn family_button(
    view: &mut BoardView,
    sel: &DesignSelection,
    window: &mut Window,
    cx: &mut Context<BoardView>,
) -> AnyElement {
    let state = view.family_list(window, cx);
    let (family, placeholder) = match sel.get("fontFamily") {
        Mixed::Same(Value::String(family)) => {
            let family = SharedString::from(family);
            (Some(family.clone()), family)
        }
        Mixed::Mixed => (None, "Mixed".into()),
        Mixed::Same(_) | Mixed::Absent => (None, SharedString::default()),
    };
    view.show_family(family, window, cx);
    div()
        .debug_selector(|| "design-fontFamily".into())
        .w_full()
        .child(
            Select::new(&state)
                .small()
                .placeholder(placeholder)
                .search_placeholder("Search fonts")
                .menu_max_h(px(320.)),
        )
        .into_any_element()
}

impl BoardView {
    /// The font family list, made the first time it is drawn, and listed again whenever the
    /// fonts have changed since. A pick sets the family of the selected texts.
    fn family_list(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<SelectState<FamilyItems>> {
        let generation = figcraft_text::layout_generation();
        if let Some(list) = &mut self.design.families {
            if list.generation != generation {
                let items = family_items();
                list.state
                    .update(cx, |state, cx| state.set_items(items, window, cx));
                list.generation = generation;
                list.shown = None;
            }
            return list.state.clone();
        }
        let state =
            cx.new(|cx| SelectState::new(family_items(), None, window, cx).searchable(true));
        let picked = cx.subscribe_in(
            &state,
            window,
            |view, _, event: &SelectEvent<FamilyItems>, _, cx| {
                if let SelectEvent::Confirm(Some(family)) = event {
                    let family = family.to_string();
                    view.edit_layers(|sel| Some(sel.set("fontFamily", json!(family))), cx);
                }
            },
        );
        // The list keeps the keyboard on its button when it closes on a pick, Escape or a cancel
        // (gpui-component's `Select`): it goes back to the canvas, whose keys would be dead
        // until it was clicked. A list closed by a click elsewhere leaves the keyboard there.
        let closed = cx.subscribe_in(
            &state,
            window,
            |view, state, _: &DismissEvent, window, cx| {
                if state.read(cx).focus_handle(cx).is_focused(window) {
                    window.focus(&view.focus, cx);
                }
            },
        );
        self.design.families = Some(Families {
            state: state.clone(),
            generation,
            shown: None,
            _events: [picked, closed],
        });
        state
    }

    /// Sets the font family list to show `family`, the texts' family: picked in the list, or
    /// none picked when they differ, or when the fonts don't have it (the button then shows its
    /// name). Done only when it changed, as it also clears what was typed in the list's search.
    fn show_family(
        &mut self,
        family: Option<SharedString>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(list) = &mut self.design.families else {
            return;
        };
        if list.shown == family {
            return;
        }
        list.shown = family.clone();
        list.state.update(cx, |state, cx| match &family {
            Some(family) => state.set_selected_value(family, window, cx),
            None => state.set_selected_index(None, window, cx),
        });
    }
}

/// The families the fonts have, as the list offers them.
fn family_items() -> FamilyItems {
    let names = guarded("ui.fonts", Vec::new(), families);
    SearchableVec::new(
        names
            .into_iter()
            .map(SharedString::from)
            .collect::<Vec<_>>(),
    )
}

/// The font style menu: the styles the texts' family has, theirs checked. Off while the texts'
/// families differ.
fn style_menu(sel: &DesignSelection, view: &WeakEntity<BoardView>) -> AnyElement {
    let family = match sel.get("fontFamily") {
        Mixed::Same(Value::String(family)) => Some(family),
        _ => None,
    };
    let style = match sel.get("fontStyle") {
        Mixed::Same(Value::String(style)) => Some(style),
        _ => None,
    };
    let label = style.clone().unwrap_or_else(|| "Mixed".to_string());
    let off = family.is_none();
    let choices = move || {
        let Some(family) = &family else {
            return Vec::new();
        };
        let named = guarded("ui.fonts", Vec::new(), || styles(family));
        named
            .into_iter()
            .map(|style| (style.clone(), style.into()))
            .collect()
    };
    let view = view.clone();
    let pick = move |style: &String, cx: &mut App| {
        let style = style.clone();
        let _ = view.update(cx, |view, cx| {
            view.edit_layers(|sel| Some(sel.set("fontStyle", json!(style))), cx);
        });
    };
    let choices = Choices::new(choices, style, pick);
    choice_menu("fontStyle", label, "Font style", choices, off).into_any_element()
}

/// The buttons that set property `key` of the texts to one of `choices`; the one the texts all
/// hold is marked. Each is drawn as `design-<key>-<value>`.
fn buttons(
    key: &'static str,
    choices: &Buttons,
    sel: &DesignSelection,
    view: &WeakEntity<BoardView>,
) -> Div {
    let current = sel.get(key);
    h_flex()
        .gap_1()
        .children(choices.iter().map(|&(value, icon, tooltip)| {
            let selector = format!("design-{key}-{value}");
            icon_button(SharedString::from(selector.clone()), icon, tooltip)
                .selected(current == Mixed::Same(json!(value)))
                .debug_selector(move || selector)
                .on_click(on_click_edit(view, move |sel| {
                    Some(sel.set(key, json!(value)))
                }))
        }))
}

/// The rule of the line height or letter spacing field, `key`: the texts' value as the engine
/// labels it ("Auto", "24", "150%"), and what `parse` makes of typed text, set on the texts.
fn spacing_rule(key: &'static str, parse: fn(&str) -> Option<Value>) -> Rule {
    Rule::new(
        move |sel| match sel.get(key) {
            Mixed::Same(Value::String(label)) => label.into(),
            Mixed::Same(Value::Number(number)) => number.to_string().into(),
            Mixed::Same(_) | Mixed::Mixed => "Mixed".into(),
            Mixed::Absent => SharedString::default(),
        },
        move |sel, typed| parse(typed).map(|value| sel.set(key, value)),
    )
}

/// What a typed line height means for `node.set` (ruling P5-C8): `auto` in any case, a number of
/// pixels, or a percentage of the font size (`150%`). `None` for anything else, a number that
/// isn't finite, and a height below 0.
fn parse_line_height(typed: &str) -> Option<Value> {
    let typed = typed.trim();
    if typed.eq_ignore_ascii_case("auto") {
        return Some(json!("auto"));
    }
    let (number, percent) = split_percent(typed);
    let number = number.filter(|height| *height >= 0.0)?;
    Some(spacing_value(number, percent))
}

/// What a typed letter spacing means for `node.set` (ruling P5-C8): a number of pixels or a
/// percentage of the font size (`2%`), either below 0 or not. `None` for anything else and a
/// number that isn't finite.
fn parse_letter_spacing(typed: &str) -> Option<Value> {
    let (number, percent) = split_percent(typed.trim());
    Some(spacing_value(number?, percent))
}

/// The finite number `typed` spells, with or without a `%` after it, and whether it has one.
fn split_percent(typed: &str) -> (Option<f64>, bool) {
    let (digits, percent) = match typed.strip_suffix('%') {
        Some(digits) => (digits.trim(), true),
        None => (typed, false),
    };
    let number = digits.parse::<f64>().ok().filter(|n| n.is_finite());
    (number.map(|n| n + 0.0), percent)
}

/// `number` as `node.set` takes a spacing: a percentage as its label (`"150%"`), else a number of
/// pixels.
fn spacing_value(number: f64, percent: bool) -> Value {
    if percent {
        json!(format!("{number}%"))
    } else {
        json!(number)
    }
}

#[cfg(test)]
mod tests {
    use figcraft_doc::NodeId;
    use gpui_kit::{Entity, TestAppContext, VisualTestContext};
    use sawit_board::design::Mixed;
    use sawit_board::design::typography::families;
    use serde_json::{Value, json};

    // Not `super::*`: gpui-kit's `test` attribute would come with it, in place of the standard one.
    use super::super::tests::{
        Design, Scratch, click, field_text, focus_canvas, open_in_window, panel_value, prop,
        rectangle,
    };
    use super::{parse_letter_spacing, parse_line_height};
    use crate::board::BoardView;

    /// Writes a design file at `path` with a text reading "Hello" and a rectangle, and returns
    /// the text's id and the rectangle's.
    fn text_and_rectangle(path: &std::path::Path) -> [NodeId; 2] {
        let mut design = Design::new();
        let text = json!({"type": "text", "text": "Hello", "x": 0, "y": 0});
        let ids = [design.make(text), design.make(rectangle(150.0))];
        design.save(path);
        ids
    }

    /// Types `text` over what field `selector` shows, and presses Enter.
    fn type_in(cx: &mut VisualTestContext, selector: &'static str, text: &str) {
        click(cx, selector);
        // A field that has the keyboard already keeps its caret where the click put it.
        cx.simulate_keystrokes("cmd-a");
        cx.simulate_input(text);
        cx.simulate_keystrokes("enter");
    }

    /// The tool the canvas has on.
    fn tool(view: &Entity<BoardView>, cx: &VisualTestContext) -> &'static str {
        view.read_with(cx, |view, _| view.board().unwrap().session.tool.tool.id())
    }

    /// The Typography section, with a text selected beside a rectangle: the fields and buttons
    /// set the text, the rectangle keeps what it has, and the line height and letter spacing take
    /// what ruling P5-C8 allows.
    #[gpui_kit::test]
    fn the_typography_section_edits_the_texts(cx: &mut TestAppContext) {
        let tmp = Scratch::new("typography");
        let path = tmp.0.join("design.figcraft");
        let [text, shape] = text_and_rectangle(&path);
        let (view, mut cx) = open_in_window(cx, &path, &[text, shape]);
        let value = |key: &str, cx: &VisualTestContext| prop(&view, text, key, cx);
        let before = prop(&view, shape, "fills", &cx);
        type_in(&mut cx, "design-field-fontSize", "24");
        assert_eq!(value("fontSize", &cx), json!(24.0));
        type_in(&mut cx, "design-field-lineHeight", "150%");
        assert_eq!(value("lineHeight", &cx), json!("150%"));
        type_in(&mut cx, "design-field-lineHeight", "abc");
        assert_eq!(value("lineHeight", &cx), json!("150%"));
        assert_eq!(field_text(&view, "lineHeight", &cx), "150%");
        type_in(&mut cx, "design-field-lineHeight", "auto");
        assert_eq!(value("lineHeight", &cx), json!("Auto"));
        type_in(&mut cx, "design-field-letterSpacing", "2");
        assert_eq!(value("letterSpacing", &cx), json!("2"));
        type_in(&mut cx, "design-field-letterSpacing", "-1%");
        assert_eq!(value("letterSpacing", &cx), json!("-1%"));
        click(&mut cx, "design-textAlign-center");
        assert_eq!(value("textAlign", &cx), json!("center"));
        click(&mut cx, "design-textAlignVertical-bottom");
        let vertical = panel_value(&view, &[text], "textAlignVertical", &cx);
        assert_eq!(vertical, Mixed::Same(json!("bottom")));
        click(&mut cx, "design-textAutoResize-height");
        assert_eq!(value("textAutoResize", &cx), json!("height"));
        assert_eq!(prop(&view, shape, "fills", &cx), before);
        assert_eq!(view.read_with(&cx, |view, _| view.toast.clone()), None);
    }

    /// The font family list takes the keyboard for its search, and gives it back to the canvas
    /// once a family is picked, so the canvas's keys work with no click on it (delegate ruling on
    /// P5.T4).
    #[gpui_kit::test]
    fn a_family_picked_from_the_list_gives_the_keyboard_back(cx: &mut TestAppContext) {
        // The system's fonts, which the list offers once they are scanned.
        figcraft_text::scan_system_fonts();
        let wanted = families()
            .into_iter()
            .find(|family| family.to_lowercase().contains("helvetica"))
            .expect("a Mac has Helvetica");
        let tmp = Scratch::new("typography-family");
        let path = tmp.0.join("design.figcraft");
        let [text, _] = text_and_rectangle(&path);
        let (view, mut cx) = open_in_window(cx, &path, &[text]);
        focus_canvas(&view, &mut cx);
        let before = tool(&view, &cx);
        click(&mut cx, "design-fontFamily");
        // Typed into the list's search: no canvas key.
        cx.simulate_input("helvetica");
        assert_eq!(tool(&view, &cx), before);
        cx.simulate_keystrokes("enter");
        assert_eq!(prop(&view, text, "fontFamily", &cx), Value::from(wanted));
        cx.simulate_keystrokes("r");
        assert_eq!(tool(&view, &cx), "rectangle");
    }

    /// Ruling P5-C8: a line height is auto, a number or a percentage, and nothing else.
    #[test]
    fn a_line_height_is_auto_a_number_or_a_percentage() {
        assert_eq!(parse_line_height("auto"), Some(json!("auto")));
        assert_eq!(parse_line_height(" Auto "), Some(json!("auto")));
        assert_eq!(parse_line_height("24"), Some(json!(24.0)));
        assert_eq!(parse_line_height("24.5"), Some(json!(24.5)));
        assert_eq!(parse_line_height("150%"), Some(json!("150%")));
        assert_eq!(parse_line_height("150 %"), Some(json!("150%")));
        assert_eq!(parse_line_height("0"), Some(json!(0.0)));
        for bad in ["", "abc", "-5", "-50%", "1e400", "inf%", "24px", "Mixed"] {
            assert_eq!(parse_line_height(bad), None, "{bad}");
        }
    }

    /// Ruling P5-C8: a letter spacing is a number or a percentage, either sign.
    #[test]
    fn a_letter_spacing_is_a_number_or_a_percentage() {
        assert_eq!(parse_letter_spacing("2"), Some(json!(2.0)));
        assert_eq!(parse_letter_spacing("-0.5"), Some(json!(-0.5)));
        assert_eq!(parse_letter_spacing("2%"), Some(json!("2%")));
        assert_eq!(parse_letter_spacing("-1.5%"), Some(json!("-1.5%")));
        assert_eq!(parse_letter_spacing("-0"), Some(json!(0.0)));
        for bad in ["", "auto", "abc", "NaN", "1e400%", "Mixed"] {
            assert_eq!(parse_letter_spacing(bad), None, "{bad}");
        }
    }
}
