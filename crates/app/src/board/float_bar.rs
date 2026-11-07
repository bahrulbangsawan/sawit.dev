//! The canvas's one floating bar (B16): the bar over a selected screen, artboard, flow or route
//! frame, built the same way so they can't drift apart (design: `docs/design/b16-popups.sawit`).
//!
//! - **Size:** exactly as wide as its buttons, its dividers and its padding, never a fixed width.
//!   The widths are worked out from the labels' shaped text ([`FloatBar::measure`]), so the bar is
//!   placed, centred over its frame and clamped to the canvas (`model::clamp_anchor`) on the frame
//!   it first draws, and a test can check there is no blank space at its ends.
//! - **Spacing:** [`HEIGHT`] 36 px with 4 px from its edge to its 28 px buttons (a 1 px border and
//!   3 px of padding), 2 px between buttons, 16 px dividers with 4 px either side.
//! - **Buttons:** a 14 px icon, 6 px, the label, and a caret on a button that opens a menu. A
//!   canvas too narrow for the bar gets the icons alone, 28 px square, each label in its button's
//!   tooltip and the name VoiceOver reads, instead of a row that scrolls.
//! - **Toggles** (Full Page, Live) show their state with an accent icon, a check after the label
//!   and the selected fill, and announce on or off to VoiceOver ([`Button::toggled`]).
//! - **One primary action** per bar at most ([`BarButton::primary`]): Send to Agent.

use std::rc::Rc;

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::*;
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenu};
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use sawit_board::context_bar as model;

/// The bar's height: its 28 px buttons, 3 px of padding and a 1 px border above and below.
pub(in crate::board) const HEIGHT: f64 = 36.;
/// The bar's corner radius: the buttons' 6 px plus the 4 px between them and its edge.
pub(in crate::board) const RADIUS: f32 = 10.;
/// The bar's border and padding at each end.
const BORDER: f32 = 1.;
const PADDING: f32 = 3.;
/// A button's height, the side of a button that shows its icon alone, and its corner radius.
pub(in crate::board) const BUTTON: f32 = 28.;
const BUTTON_RADIUS: f32 = 6.;
/// The room between two buttons, and a divider's width and margin (which with the gaps beside
/// it makes 4 px either side of the line).
const GAP: f32 = 2.;
const DIVIDER: f32 = 1.;
const DIVIDER_MARGIN: f32 = 2.;
const DIVIDER_HEIGHT: f32 = 16.;
/// Inside a button with words: its side padding, the icon, the room between the icon and the
/// words, and the caret of a menu button with the room before it.
const SIDE: f32 = 8.;
const ICON: f32 = 14.;
const ICON_GAP: f32 = 6.;
const CARET: f32 = 12.;
const CARET_GAP: f32 = 4.;
/// The room either side of a [`FloatBar::text`].
const TEXT_SIDE: f32 = 6.;
/// The room between the frame's top and the bar's bottom, for the frame's name.
pub(in crate::board) const ABOVE: f64 = 34.;

type Click = Rc<dyn Fn(&ClickEvent, &mut Window, &mut App)>;
type Menu = Rc<dyn Fn(PopupMenu, &mut Window, &mut Context<PopupMenu>) -> PopupMenu>;

/// One action on a [`FloatBar`]: an icon and a label, a click or a menu.
pub(in crate::board) struct BarButton {
    id: &'static str,
    icon: Lucide,
    label: SharedString,
    tooltip: Option<SharedString>,
    primary: bool,
    toggle: Option<bool>,
    disabled: bool,
    click: Option<Click>,
    menu: Option<Menu>,
}

impl BarButton {
    /// A button `id` (its debug selector too) with `icon` and `label`.
    pub(in crate::board) fn new(
        id: &'static str,
        icon: Lucide,
        label: impl Into<SharedString>,
    ) -> Self {
        Self {
            id,
            icon,
            label: label.into(),
            tooltip: None,
            primary: false,
            toggle: None,
            disabled: false,
            click: None,
            menu: None,
        }
    }

    /// What the button's tooltip says beyond its label.
    pub(in crate::board) fn tooltip(mut self, tooltip: impl Into<SharedString>) -> Self {
        self.tooltip = Some(tooltip.into());
        self
    }

    /// The bar's one primary action, in the primary button style.
    pub(in crate::board) fn primary(mut self) -> Self {
        self.primary = true;
        self
    }

    /// A toggle, `on` or off.
    pub(in crate::board) fn toggle(mut self, on: bool) -> Self {
        self.toggle = Some(on);
        self
    }

    pub(in crate::board) fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub(in crate::board) fn on_click(
        mut self,
        click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.click = Some(Rc::new(click));
        self
    }

    /// A button that opens the menu `menu` fills, under its left edge, at least as wide as it.
    pub(in crate::board) fn menu(
        mut self,
        menu: impl Fn(PopupMenu, &mut Window, &mut Context<PopupMenu>) -> PopupMenu + 'static,
    ) -> Self {
        self.menu = Some(Rc::new(menu));
        self
    }

    /// Whether the button is on, for a toggle.
    fn on(&self) -> bool {
        self.toggle == Some(true)
    }

    /// The button's width with its words, `text` the width of its label: its padding, icon,
    /// label, and its caret or its check when it has one.
    fn full_width(&self, text: f32) -> f32 {
        let mut width = SIDE + ICON + ICON_GAP + text + SIDE;
        if self.menu.is_some() {
            width += CARET_GAP + CARET;
        }
        if self.on() {
            width += ICON_GAP + ICON;
        }
        width
    }
}

enum Part {
    Button(BarButton),
    /// Words that aren't a button (the timer's time), semibold, in the danger colour if `alert`.
    Text {
        id: &'static str,
        text: SharedString,
        alert: bool,
    },
    Divider,
}

/// A floating bar over a frame on the canvas: its buttons and the dividers between their groups.
pub(in crate::board) struct FloatBar {
    id: &'static str,
    parts: Vec<Part>,
}

/// How a bar is laid out: each part's width (0 for a divider, which takes [`divider_width`]),
/// the bar's width, and whether its labels are collapsed to icons.
#[derive(Clone, Debug, PartialEq)]
pub(in crate::board) struct Measured {
    pub widths: Vec<f32>,
    pub width: f32,
    pub collapsed: bool,
}

/// The room a divider takes in the row, without the gaps beside it.
fn divider_width() -> f32 {
    DIVIDER + 2.0 * DIVIDER_MARGIN
}

/// The width of a bar of parts `widths` (a divider's given as `None`): the parts, the gaps
/// between them, its padding and border.
fn bar_width(widths: &[Option<f32>]) -> f32 {
    let parts: f32 = widths.iter().map(|w| w.unwrap_or(divider_width())).sum();
    let gaps = widths.len().saturating_sub(1) as f32 * GAP;
    parts + gaps + 2.0 * (PADDING + BORDER)
}

/// How wide `text` is in a small button's text (`text_sm`), as the button draws it.
fn text_width(text: &SharedString, window: &mut Window) -> f32 {
    let style = window.text_style();
    let size = rems(0.875).to_pixels(window.rem_size());
    let run = TextRun {
        len: text.len(),
        font: style.font(),
        color: style.color,
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    let width = window
        .text_system()
        .shape_line(text.clone(), size, &[run], None)
        .width;
    // Whole pixels, rounded up, so a label never ellipsises by a fraction.
    f32::from(width).ceil()
}

impl FloatBar {
    /// A bar `id` (its debug selector too), empty.
    pub(in crate::board) fn new(id: &'static str) -> Self {
        Self {
            id,
            parts: Vec::new(),
        }
    }

    pub(in crate::board) fn button(mut self, button: BarButton) -> Self {
        self.parts.push(Part::Button(button));
        self
    }

    /// Words that aren't a button, such as the timer's time: semibold, with tabular figures, in
    /// the danger colour when `alert`.
    pub(in crate::board) fn text(
        mut self,
        id: &'static str,
        text: impl Into<SharedString>,
        alert: bool,
    ) -> Self {
        self.parts.push(Part::Text {
            id,
            text: text.into(),
            alert,
        });
        self
    }

    /// A line between two groups of buttons. One at either end, or two side by side, is dropped.
    pub(in crate::board) fn divider(mut self) -> Self {
        if matches!(self.parts.last(), Some(Part::Button(_) | Part::Text { .. })) {
            self.parts.push(Part::Divider);
        }
        self
    }

    /// Lays the bar out for a canvas with `room` px across for it: each button at its full width
    /// while the whole bar fits, else every button as its icon alone.
    pub(in crate::board) fn measure(&self, room: f32, window: &mut Window) -> Measured {
        let full: Vec<Option<f32>> = self
            .parts
            .iter()
            .map(|part| match part {
                Part::Button(b) => Some(b.full_width(text_width(&b.label, window))),
                // Tabular figures may run a little wider than the default ones measured: 2 px spare.
                Part::Text { text, .. } => Some(text_width(text, window) + 2.0 * TEXT_SIDE + 2.0),
                Part::Divider => None,
            })
            .collect();
        let width = bar_width(&full);
        if width <= room {
            return Measured {
                widths: full.iter().map(|w| w.unwrap_or(0.0)).collect(),
                width,
                collapsed: false,
            };
        }
        let icons: Vec<Option<f32>> = (self.parts.iter().zip(&full))
            .map(|(part, w)| match part {
                Part::Button(_) => Some(BUTTON),
                _ => *w,
            })
            .collect();
        Measured {
            widths: icons.iter().map(|w| w.unwrap_or(0.0)).collect(),
            width: bar_width(&icons),
            collapsed: true,
        }
    }

    /// The bar, its top centre at `top_centre` in the canvas `canvas` (width, height), clamped
    /// inside it.
    pub(in crate::board) fn render(
        self,
        top_centre: (f64, f64),
        canvas: (f64, f64),
        window: &mut Window,
        cx: &App,
    ) -> Stateful<Div> {
        let room = (canvas.0 - 2.0 * model::MARGIN).max(0.0) as f32;
        let (bar, measured) = self.draw(room, window, cx);
        let width = f64::from(measured.width);
        let (x, y) = model::clamp_anchor(top_centre, (width, HEIGHT), canvas);
        bar.absolute()
            .left(px((x - width / 2.0) as f32))
            .top(px(y as f32))
    }

    /// The bar where its parent puts it (the timer and vote pill in the canvas's corner), with
    /// `room` px across for it.
    pub(in crate::board) fn render_in_place(
        self,
        room: f32,
        window: &mut Window,
        cx: &App,
    ) -> Stateful<Div> {
        self.draw(room, window, cx).0
    }

    /// The bar's box and buttons, laid out for `room` px across, and how it was measured.
    fn draw(mut self, room: f32, window: &mut Window, cx: &App) -> (Stateful<Div>, Measured) {
        while matches!(self.parts.last(), Some(Part::Divider)) {
            self.parts.pop();
        }
        let measured = self.measure(room, window);
        let theme = cx.theme();
        let mut row = h_flex().gap(px(GAP)).h_full().items_center();
        for (part, w) in self.parts.into_iter().zip(measured.widths.iter().copied()) {
            row = match part {
                Part::Divider => row.child(
                    div()
                        .flex_none()
                        .w(px(DIVIDER))
                        .h(px(DIVIDER_HEIGHT))
                        .mx(px(DIVIDER_MARGIN))
                        .bg(theme.border),
                ),
                Part::Button(button) => row.child(render_button(button, w, measured.collapsed, cx)),
                Part::Text { id, text, alert } => row.child(
                    div()
                        .id(id)
                        .debug_selector(move || id.to_owned())
                        .flex_none()
                        .w(px(w))
                        .px(px(TEXT_SIDE))
                        .text_sm()
                        .font_semibold()
                        .font_features(FontFeatures(std::sync::Arc::new(vec![("tnum".into(), 1)])))
                        .whitespace_nowrap()
                        .text_color(if alert {
                            theme.danger
                        } else {
                            theme.popover_foreground
                        })
                        .child(text),
                ),
            };
        }
        let id = self.id;
        let bar = div()
            .id(id)
            .debug_selector(move || id.to_owned())
            // Its clicks and scrolls are its own: none reaches the canvas under it.
            .occlude()
            .flex_none()
            .w(px(measured.width))
            .h(px(HEIGHT as f32))
            .p(px(PADDING))
            .rounded(px(RADIUS))
            .border(px(BORDER))
            .border_color(theme.border)
            .bg(theme.popover)
            .text_color(theme.popover_foreground)
            .shadow_md()
            .child(row);
        (bar, measured)
    }
}

/// `button`, `width` wide, as its icon alone when `collapsed`.
fn render_button(button: BarButton, width: f32, collapsed: bool, cx: &App) -> AnyElement {
    let theme = cx.theme();
    let on = button.on();
    let icon_color = if button.disabled {
        theme.muted_foreground.opacity(0.5)
    } else if button.primary {
        theme.primary_foreground
    } else if on {
        theme.primary
    } else {
        theme.muted_foreground
    };
    let label = button.label.clone();
    let tooltip = match (&button.tooltip, collapsed) {
        (Some(tip), true) => SharedString::from(format!("{label}: {tip}")),
        (Some(tip), false) => tip.clone(),
        (None, _) => label.clone(),
    };
    let caret_color = if button.primary {
        theme.primary_foreground.opacity(0.8)
    } else {
        theme.muted_foreground
    };
    let content = h_flex()
        .gap(px(ICON_GAP))
        .items_center()
        .child(Icon::new(button.icon).size(px(ICON)).text_color(icon_color))
        .when(!collapsed, |this| {
            this.child(div().whitespace_nowrap().child(label.clone()))
                .when(on, |this| {
                    this.child(
                        Icon::new(Lucide::Check)
                            .size(px(ICON))
                            .text_color(theme.primary),
                    )
                })
                .when(button.menu.is_some(), |this| {
                    this.child(
                        Icon::new(Lucide::ChevronDown)
                            .size(px(CARET))
                            .ml(px(CARET_GAP - ICON_GAP))
                            .text_color(caret_color),
                    )
                })
        });
    let id = button.id;
    let mut b = Button::new(id)
        .small()
        .map(|b| {
            if button.primary {
                b.primary()
            } else {
                b.ghost()
            }
        })
        .child(content)
        .w(px(width))
        .h(px(BUTTON))
        .px(px(if collapsed { 0. } else { SIDE }))
        .rounded(px(BUTTON_RADIUS))
        .accessibility_label(label)
        .tooltip(tooltip)
        .disabled(button.disabled)
        .debug_selector(move || id.to_owned());
    if let Some(on) = button.toggle {
        b = b.selected(on).toggled(on);
    }
    if let Some(click) = button.click {
        b = b.on_click(move |event, window, cx| click(event, window, cx));
    }
    match button.menu {
        Some(menu) => b
            .dropdown_menu(move |popup, window, cx| menu(popup.min_w(px(width)), window, cx))
            .into_any_element(),
        None => b.into_any_element(),
    }
}

#[cfg(test)]
mod tests {
    use gpui_kit::assets::IconName as Lucide;

    use super::{BarButton, bar_width};

    #[test]
    fn a_bar_is_its_parts_gaps_and_padding() {
        // Two buttons, a divider, a button: three gaps.
        let w = bar_width(&[Some(60.), Some(70.), None, Some(80.)]);
        assert_eq!(w, 60. + 70. + 5. + 80. + 3. * 2. + 2. * 4.);
    }

    #[test]
    fn a_button_with_words_fits_them() {
        let b = BarButton::new("b", Lucide::Eye, "Preview");
        assert_eq!(b.full_width(50.), 8. + 14. + 6. + 50. + 8.);
        let menu = BarButton::new("m", Lucide::Bot, "Send").menu(|m, _, _| m);
        assert_eq!(menu.full_width(50.), 8. + 14. + 6. + 50. + 4. + 12. + 8.);
        let on = BarButton::new("t", Lucide::UnfoldVertical, "Full Page").toggle(true);
        assert_eq!(on.full_width(50.), 8. + 14. + 6. + 50. + 6. + 14. + 8.);
        let off = BarButton::new("t", Lucide::UnfoldVertical, "Full Page").toggle(false);
        assert_eq!(off.full_width(50.), 8. + 14. + 6. + 50. + 8.);
    }
}
