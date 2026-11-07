//! The Design panel for an HTML screen (B15): only what applies to it. Its page covers the frame,
//! so fill, stroke, auto layout, resizing, layout grids and type would change nothing there; the
//! panel shows the Position section, then a Screen section: what it is, its size as a device
//! (D1's presets, grouped by kind), and its actions: Convert to Layers (the one primary action:
//! it makes the screen editable here), Preview and Export.

use std::rc::Rc;

use figcraft_doc::NodeId;
use gpui_kit::component::button::*;
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_kit::component::*;
use gpui_kit::*;
use sawit_board::design::DesignSelection;
use sawit_core::browser::DEVICES;
use serde_json::json;

use super::controls::{row, section, set_props};
use super::number_field::NumberField;
use crate::board::BoardView;
use crate::board::screens::{device_group, device_icon};
use crate::menu_icons;

/// What the Screen section says a screen is, and how to edit it.
pub(super) const ABOUT: &str =
    "Its content is a web page. Convert it to layers to edit each text and button here.";

/// The size a screen frame `w` × `h` is, as its menu shows it: the device of that size, or the
/// size alone.
pub(super) fn size_label(w: f64, h: f64) -> String {
    let (w, h) = (w.round() as u32, h.round() as u32);
    match DEVICES.iter().find(|d| d.width == w && d.height == h) {
        Some(d) => format!("{} · {w} × {h}", d.name),
        None => format!("Custom · {w} × {h}"),
    }
}

/// The Screen section of screen frame `frame` (`sel` is it alone).
pub(super) fn render(
    view: &mut BoardView,
    sel: &Rc<DesignSelection>,
    frame: NodeId,
    cx: &mut Context<BoardView>,
) -> AnyElement {
    let theme = cx.theme().clone();
    let this = cx.entity().downgrade();
    let size = (view.board())
        .and_then(|b| b.session.ds().ok())
        .and_then(|ds| ds.doc.node(frame).map(|n| (n.width, n.height)))
        .unwrap_or((0.0, 0.0));
    let label = size_label(size.0, size.1);
    let size_menu = {
        let sel = sel.clone();
        let this = this.clone();
        let current = (size.0.round() as u32, size.1.round() as u32);
        Button::new("design-screen-size")
            .outline()
            .small()
            .w_full()
            .icon(menu_icons::DEVICE)
            .label(label.clone())
            .dropdown_caret(true)
            .tooltip("The size the page is laid out at")
            .accessibility_label(format!("Size: {label}"))
            .debug_selector(|| "design-screen-size".into())
            .dropdown_menu(move |mut menu, _, _| {
                let mut group = "";
                for d in DEVICES {
                    let of = device_group(d.width);
                    if of != group {
                        if !group.is_empty() {
                            menu = menu.separator();
                        }
                        menu = menu.label(of);
                        group = of;
                    }
                    let (sel, this) = (sel.clone(), this.clone());
                    let (w, h) = (d.width, d.height);
                    menu = menu.item(
                        PopupMenuItem::new(format!("{} · {} × {}", d.name, w, h))
                            .icon(device_icon(d.name, d.width))
                            .checked(current == (w, h))
                            .on_click(move |_, _, cx| {
                                let edit = set_props(&sel, json!({"width": w, "height": h}));
                                let _ = this.update(cx, |view, cx| view.apply_edit(edit, cx));
                            }),
                    );
                }
                menu
            })
    };
    let convert = {
        let this = this.clone();
        Button::new("design-screen-layers")
            .primary()
            .small()
            .w_full()
            .icon(menu_icons::LAYERS)
            .label("Convert to Layers")
            .tooltip("Make an editable copy from layers, beside the screen")
            .debug_selector(|| "design-screen-layers".into())
            .on_click(move |_, _, cx| {
                let _ = this.update(cx, |view, cx| view.convert_screen_to_layers(frame, cx));
            })
    };
    let preview = {
        let this = this.clone();
        Button::new("design-screen-preview")
            .small()
            .flex_1()
            .icon(menu_icons::PREVIEW)
            .label("Preview")
            .tooltip("Preview the screen full size, live")
            .on_click(move |_, window, cx| {
                let _ = this.update(cx, |view, cx| view.open_preview(frame, window, cx));
            })
    };
    let export = {
        let this = this.clone();
        Button::new("design-screen-export")
            .small()
            .flex_1()
            .icon(menu_icons::EXPORT)
            .label("Export")
            .dropdown_caret(true)
            .tooltip("Save the screen as code or a picture")
            .dropdown_menu(move |menu, _, _| {
                let code = |react: bool| {
                    let this = this.clone();
                    move |_: &ClickEvent, window: &mut Window, cx: &mut App| {
                        let _ =
                            this.update(cx, |view, cx| view.export_code(frame, react, window, cx));
                    }
                };
                let png = {
                    let this = this.clone();
                    move |_: &ClickEvent, window: &mut Window, cx: &mut App| {
                        let _ =
                            this.update(cx, |view, cx| view.export_screen_png(frame, window, cx));
                    }
                };
                menu.item(
                    PopupMenuItem::new("HTML")
                        .icon(menu_icons::HTML)
                        .on_click(code(false)),
                )
                .item(
                    PopupMenuItem::new("React")
                        .icon(menu_icons::REACT)
                        .on_click(code(true)),
                )
                .separator()
                .item(
                    PopupMenuItem::new("PNG")
                        .icon(menu_icons::IMAGE)
                        .on_click(png),
                )
            })
    };
    let rows = vec![
        div()
            .text_xs()
            .line_height(rems(1.125))
            .text_color(theme.muted_foreground)
            .child(ABOUT)
            .into_any_element(),
        row()
            .child(div().flex_1().min_w_0().child(size_menu))
            .into_any_element(),
        row().child(convert).into_any_element(),
        row().child(preview).child(export).into_any_element(),
    ];
    section("screen", "Screen", Vec::new(), rows, cx)
}

/// A screen's Position section: X and Y, W and H. Rotating or aligning a screen's frame doesn't
/// turn or move its page's layout, so those stay out.
pub(super) fn position(
    view: &mut BoardView,
    sel: &Rc<DesignSelection>,
    window: &mut Window,
    cx: &mut Context<BoardView>,
) -> AnyElement {
    let x = NumberField::new("x", "X").render(view, sel, window, cx);
    let y = NumberField::new("y", "Y").render(view, sel, window, cx);
    let w = NumberField::new("width", "W").render(view, sel, window, cx);
    let h = NumberField::new("height", "H").render(view, sel, window, cx);
    let rows = vec![
        row().child(x).child(y).into_any_element(),
        row().child(w).child(h).into_any_element(),
    ];
    section("position", "Position", Vec::new(), rows, cx)
}
