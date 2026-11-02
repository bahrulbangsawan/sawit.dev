//! The Design panel's fields: a label beside an input. A [`NumberField`] edits one number property
//! of the layers, and its label scrubs it. A [`TextField`] edits anything else that is typed, by a
//! [`Rule`] of its own: a paint's hex or opacity, a shadow's offset, a line height.
//!
//! - **Typing.** What is typed goes to every selected layer on Enter, or when the field loses the
//!   keyboard: a number field's through `sawit_board::design::parse_number`, a text field's through
//!   its rule. Input the field refuses changes nothing, and the field shows the layers' value
//!   again, as it does after an edit the engine refuses. The layers the value goes to are the ones
//!   on show when the field took the keyboard, as they are when it is applied: another selection
//!   (an agent's, the layers panel's) may come before the field hears that it lost the keyboard.
//!   A press on the canvas applies the value before the tools have the press, so that it is an
//!   undo step of its own and no gesture the press starts takes it in or back. The text is
//!   selected when the field takes the keyboard, so what is typed replaces it.
//! - **Scrubbing.** A number field's label is a drag handle: dragging it sideways changes the value
//!   by 1 per pixel (an opacity by 1%), ten times as fast with ⇧ held, and the whole drag is one
//!   undo step. Escape puts the value back ([`BoardView::scrub_begin`]). The drag's moves and its
//!   release are heard on the whole window ([`super::track_scrub`]).
//! - **Showing.** Layers whose values differ show "Mixed", and a value typed or scrubbed there goes
//!   to all of them. An opacity shows as a percentage, and takes one: a number typed without `%` is
//!   read as one (ruling P5-C7).

mod label_scrub;
mod typing;

use std::rc::Rc;

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use sawit_board::design::{DesignSelection, Edit, Mixed, parse_number};
use serde_json::json;

pub(super) use label_scrub::LabelDrag;

use super::controls::guarded;
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

/// A field of the Design panel for typed input other than one number property, named `id` among
/// the panel's fields: what it shows for the layers and what it makes of typed text are its
/// [`Rule`]'s. Its label, if it has one, doesn't scrub.
pub struct TextField {
    id: SharedString,
    label: Option<SharedString>,
    icon: Option<Lucide>,
    /// The field's width, when it isn't to share its row's width with the others.
    width: Option<f32>,
    rule: Rule,
}

/// What a field does with the layers it shows: the text it shows for them, and the edit a typed
/// text makes of them, or `None` when the field refuses the text and shows the layers' value
/// again.
#[derive(Clone)]
pub(super) struct Rule {
    show: Rc<Show>,
    apply: Rc<Apply>,
}

/// How a field shows the layers.
type Show = dyn Fn(&DesignSelection) -> SharedString;
/// The edit a field makes of the layers with typed text.
type Apply = dyn Fn(&DesignSelection, &str) -> Option<Edit>;

/// A field's input and what the panel knows of it, kept from the first time it is drawn.
pub(super) struct Field {
    pub(super) input: Entity<InputState>,
    /// The text the field was last given from the layers: typed text that differs is applied.
    shown: SharedString,
    /// The layers the field edits while it has the keyboard: those on show when it took it.
    editing: Option<Rc<DesignSelection>>,
    /// What the field does, as it was last drawn.
    rule: Rule,
    _events: Subscription,
}

impl Rule {
    /// The rule that shows the layers as `show` reads them, and makes `apply`'s edit of typed text.
    pub(super) fn new(
        show: impl Fn(&DesignSelection) -> SharedString + 'static,
        apply: impl Fn(&DesignSelection, &str) -> Option<Edit> + 'static,
    ) -> Self {
        Self {
            show: Rc::new(show),
            apply: Rc::new(apply),
        }
    }

    /// The rule of the number field of property `key`: the layers' value as [`text`] shows it, and
    /// a number typed as [`parse`] reads it, set on every layer.
    fn number(key: &'static str) -> Self {
        Self::new(
            move |sel| text(key, &sel.get(key)),
            move |sel, typed| parse(key, typed).map(|value| sel.set(key, json!(value))),
        )
    }
}

impl TextField {
    /// The field named `id`, which does what `rule` says.
    pub fn new(id: impl Into<SharedString>, rule: Rule) -> Self {
        Self {
            id: id.into(),
            label: None,
            icon: None,
            width: None,
            rule,
        }
    }

    /// Shows `label` beside the input.
    pub fn label(mut self, label: impl Into<SharedString>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// Shows `icon` in place of the label, which becomes its tooltip.
    pub fn icon(mut self, icon: Lucide) -> Self {
        self.icon = Some(icon);
        self
    }

    /// Makes the field `width` wide, instead of sharing its row's width.
    pub fn width(mut self, width: f32) -> Self {
        self.width = Some(width);
        self
    }

    /// The field as `view`'s panel draws it for the layers of `sel`. Its input shows what the rule
    /// reads, unless it has the keyboard.
    pub(super) fn render(
        self,
        view: &mut BoardView,
        sel: &DesignSelection,
        window: &mut Window,
        cx: &mut Context<BoardView>,
    ) -> AnyElement {
        let Self {
            id,
            label,
            icon,
            width,
            rule,
        } = self;
        let shown = guarded("ui.design", SharedString::default(), || (rule.show)(sel));
        let input = view.field_input(id.clone(), rule, window, cx);
        view.show_in_field(&id, shown, window, cx);
        let handle = label.map(|label| handle(&id, label, icon, cx));
        frame(&id, handle, &input, width, cx)
    }
}

/// A field's label at its input's left, or its icon with the label as the icon's tooltip.
fn handle(id: &str, label: SharedString, icon: Option<Lucide>, cx: &App) -> Stateful<Div> {
    let selector = format!("design-handle-{id}");
    div()
        .id(SharedString::from(selector.clone()))
        .debug_selector(move || selector)
        .flex_shrink_0()
        .h_full()
        .min_w(px(22.))
        .px_1p5()
        .flex()
        .items_center()
        .justify_center()
        .text_xs()
        .text_color(cx.theme().muted_foreground)
        .map(|handle| match icon {
            Some(icon) => handle
                .child(Icon::new(icon).xsmall())
                .tooltip(move |window, cx| Tooltip::new(label.clone()).build(window, cx)),
            None => handle.child(label),
        })
}

/// The frame of field `id`: its `handle`, if any, beside its `input`. `width` wide, or as wide as
/// the row leaves it.
fn frame(
    id: &str,
    handle: Option<Stateful<Div>>,
    input: &Entity<InputState>,
    width: Option<f32>,
    cx: &App,
) -> AnyElement {
    let selector = format!("design-field-{id}");
    h_flex()
        .map(|frame| match width {
            Some(width) => frame.flex_shrink_0().w(px(width)),
            None => frame.flex_1().min_w_0(),
        })
        .h(px(HEIGHT))
        .rounded(px(5.))
        .border_1()
        .border_color(cx.theme().input)
        .overflow_hidden()
        .children(handle)
        .child(
            div()
                .debug_selector(move || selector)
                .flex_1()
                .min_w_0()
                .child(Input::new(input).appearance(false).small()),
        )
        .into_any_element()
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
        let input = view.field_input(key.into(), Rule::number(key), window, cx);
        view.show_in_field(key, text(key, &value), window, cx);
        let scrubs = !matches!(value, Mixed::Absent);
        let handle = handle(key, label, icon, cx).when(scrubs, |handle| {
            handle.cursor(CursorStyle::ResizeLeftRight).on_mouse_down(
                MouseButton::Left,
                cx.listener(move |view, event: &MouseDownEvent, window, cx| {
                    // The press is the label's: nothing under it takes it.
                    cx.stop_propagation();
                    view.begin_label_scrub(key, event.position.x, window, cx);
                }),
            )
        });
        frame(key, Some(handle), &input, None, cx)
    }
}

/// How field `key` shows `value`: a number rounded to a hundredth ("100", "12.5"), an opacity as a
/// percentage ("50%"), "Mixed" when the layers differ or the value is a list (a padding or corner
/// radius whose sides differ), and nothing when no layer has the key.
pub(super) fn text(key: &str, value: &Mixed) -> SharedString {
    match value {
        Mixed::Absent => SharedString::default(),
        Mixed::Same(value) => match value.as_f64() {
            Some(opacity) if key == "opacity" => percent(opacity),
            Some(number) => number_text(number),
            None => "Mixed".into(),
        },
        Mixed::Mixed => "Mixed".into(),
    }
}

/// `number` as a field shows it: rounded to a hundredth, without a sign on zero ("100", "12.5").
pub(super) fn number_text(number: f64) -> SharedString {
    format!("{}", (number * 100.0).round() / 100.0 + 0.0).into()
}

/// A fraction as a field shows it, a whole percentage: 0.5 is "50%".
pub(super) fn percent(fraction: f64) -> SharedString {
    format!("{}%", (fraction * 100.0).round() + 0.0).into()
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

#[cfg(test)]
mod tests;
