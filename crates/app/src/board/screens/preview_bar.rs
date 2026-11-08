//! Preview's bar and stage (B15): the screen's name, then one size menu (As Made, Window, the
//! devices grouped Mobile, Tablet, Laptop and Desktop, and Custom size…, whose width and height
//! are typed in the bar), Rotate, the zoom (Fit or 100%), the device frame, Reload, Open in
//! Browser and Close; every control has an icon, and the bar never scrolls. Under it the stage:
//! the page centred, with the device frame's bezel and shadow when it is on. Nothing else is drawn
//! over Preview.

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::*;
use gpui_kit::component::input::Input;
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::*;
use gpui_kit::*;
use sawit_core::browser::DEVICES;

use super::preview::{BAR, BEZEL, BEZEL_RADIUS, Preview, Size, Zoom};
use super::{device_group, device_icon};
use crate::board::BoardView;
use crate::menu_icons;

/// Below this canvas width the size menu shows the size alone; below `TIGHT` the zoom is one
/// toggle, Close its icon, and the dividers go, so the bar never clips.
const NARROW: f32 = 760.;
const TIGHT: f32 = 600.;

/// The name of `size` in the menu, and on its button (`short` drops the device's name).
fn size_label(preview: &Preview, size: Size, short: bool) -> String {
    let (w, h) = match size {
        Size::Made => preview.made,
        Size::Window => return "Window".into(),
        Size::Device(i) => DEVICES.get(i).map_or(preview.made, |d| (d.width, d.height)),
        Size::Custom(w, h) => (w, h),
    };
    let (w, h) = match preview.rotated && size == preview.size {
        true => (h, w),
        false => (w, h),
    };
    let name = match size {
        Size::Made => "As Made",
        Size::Device(i) => DEVICES.get(i).map_or("", |d| d.name),
        _ => "Custom",
    };
    match short {
        true => format!("{w} × {h}"),
        false => format!("{name} · {w} × {h}"),
    }
}

fn size_icon(size: Size) -> Lucide {
    match size {
        Size::Made => Lucide::Frame,
        Size::Window => Lucide::AppWindow,
        Size::Device(i) => DEVICES
            .get(i)
            .map_or(Lucide::Smartphone, |d| device_icon(d.name, d.width)),
        Size::Custom(..) => menu_icons::MAXIMIZE,
    }
}

/// A ghost icon button of the bar, with its name for assistive tech and a tooltip.
fn icon_button(id: &'static str, icon: Lucide, name: &'static str, tip: &'static str) -> Button {
    Button::new(id)
        .small()
        .ghost()
        .icon(icon)
        .accessibility_label(name)
        .tooltip(tip)
}

fn divider(cx: &App) -> Div {
    div()
        .flex_shrink_0()
        .w(px(1.))
        .h(px(16.))
        .mx_1()
        .bg(cx.theme().border)
}

/// Preview's bar over the canvas and the stage under it. It takes every click over the canvas
/// while it shows.
pub(in crate::board) fn render_preview(
    view: &BoardView,
    cx: &mut Context<BoardView>,
) -> Option<impl IntoElement + use<>> {
    let preview = view.screen_ui.preview.as_ref()?;
    let this = cx.entity().downgrade();
    let theme = cx.theme().clone();
    let canvas = view.web.as_ref().map_or((1200.0, 800.0), |w| {
        (
            f32::from(w.canvas.size.width),
            f32::from(w.canvas.size.height),
        )
    });
    let narrow = canvas.0 < NARROW;
    let tight = canvas.0 < TIGHT;
    let chosen = preview.size;
    let shown = size_label(preview, chosen, narrow);
    let mut sizes = vec![Size::Made, Size::Window];
    sizes.extend((0..DEVICES.len()).map(Size::Device));
    let labels: Vec<(Size, String)> = (sizes.iter())
        .map(|s| (*s, size_label(preview, *s, false)))
        .collect();
    let size_menu = {
        let this = this.clone();
        Button::new("screen-preview-size")
            .small()
            .ghost()
            .icon(Icon::new(size_icon(chosen)).text_color(theme.muted_foreground))
            .label(shown.clone())
            .dropdown_caret(true)
            .accessibility_label(format!("Size: {}", size_label(preview, chosen, false)))
            .tooltip("Choose the size to preview at")
            .dropdown_menu(move |mut menu, _, _| {
                let mut group = "";
                for (size, label) in labels.iter().cloned() {
                    if let Size::Device(i) = size {
                        let of = DEVICES.get(i).map_or("", |d| device_group(d.width));
                        if of != group {
                            menu = menu.separator().label(of);
                            group = of;
                        }
                    }
                    let this = this.clone();
                    menu = menu.item(
                        PopupMenuItem::new(label)
                            .icon(size_icon(size))
                            .checked(size == chosen)
                            .on_click(move |_, _, cx| {
                                let _ = this.update(cx, |view, cx| {
                                    view.preview_change(|p| p.size = size, cx)
                                });
                            }),
                    );
                }
                let this = this.clone();
                menu.separator().item(
                    PopupMenuItem::new("Custom size…")
                        .icon(menu_icons::MAXIMIZE)
                        .checked(matches!(chosen, Size::Custom(..)))
                        .on_click(move |_, window, cx| {
                            let _ =
                                this.update(cx, |view, cx| view.preview_custom_start(window, cx));
                        }),
                )
            })
    };
    let size_part = match &preview.custom {
        None => size_menu.into_any_element(),
        Some(edit) => {
            let (apply, cancel) = (this.clone(), this.clone());
            let field = |input, label: &'static str| {
                h_flex()
                    .gap_1()
                    .items_center()
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .child(label),
                    )
                    .child(div().w(px(64.)).child(Input::new(input).small()))
            };
            h_flex()
                .id("screen-preview-custom")
                .gap_2()
                .items_center()
                .child(field(&edit.w, "W"))
                .child(field(&edit.h, "H"))
                .child(
                    Button::new("screen-preview-custom-apply")
                        .small()
                        .primary()
                        .label("Apply")
                        .on_click(move |_, window, cx| {
                            let _ = apply.update(cx, |v, cx| v.preview_custom_apply(window, cx));
                        }),
                )
                .child(
                    icon_button(
                        "screen-preview-custom-cancel",
                        menu_icons::CLOSE,
                        "Cancel",
                        "Cancel (Escape)",
                    )
                    .on_click(move |_, window, cx| {
                        let _ = cancel.update(cx, |v, cx| v.preview_custom_cancel(window, cx));
                    }),
                )
                .into_any_element()
        }
    };
    let rotate = {
        let this = this.clone();
        icon_button(
            "screen-preview-rotate",
            Lucide::RotateCw,
            "Rotate",
            "Turn the size on its side",
        )
        .toggled(preview.rotated)
        .selected(preview.rotated)
        .disabled(chosen == Size::Window)
        .on_click(move |_, _, cx| {
            let _ = this.update(cx, |view, cx| {
                view.preview_change(|p| p.rotated = !p.rotated, cx)
            });
        })
    };
    let fits = preview.actual_fits(canvas);
    let zoom = |id: &'static str, z: Zoom, label: &'static str| {
        let this = this.clone();
        let on = preview.zoom == z;
        let mut b = Button::new(id)
            .small()
            .ghost()
            .label(label)
            .selected(on)
            .toggled(on)
            .on_click(move |_, _, cx| {
                let _ = this.update(cx, |view, cx| view.preview_change(|p| p.zoom = z, cx));
            });
        b = match z {
            Zoom::Fit => b.tooltip("Fit the page in the window"),
            Zoom::Actual if fits => {
                b.tooltip("Show the page at its actual size; a taller page scrolls")
            }
            Zoom::Actual => b
                .disabled(true)
                .tooltip("Make the window wider to show this size at 100%"),
        };
        b
    };
    let framed = {
        let this = this.clone();
        let fits = preview.frame_fits();
        icon_button(
            "screen-preview-frame",
            Lucide::Smartphone,
            "Device frame",
            if fits {
                "Show a device frame around the page"
            } else {
                "Device frames are for phone and tablet sizes"
            },
        )
        .toggled(preview.frame_shown())
        .selected(preview.frame_shown())
        .disabled(!fits)
        .on_click(move |_, _, cx| {
            let _ = this.update(cx, |view, cx| {
                view.preview_change(|p| p.framed = !p.framed, cx)
            });
        })
    };
    let reload = {
        let this = this.clone();
        icon_button(
            "screen-preview-reload",
            menu_icons::REFRESH,
            "Reload",
            "Reload the page",
        )
        .on_click(move |_, _, cx| {
            let _ = this.update(cx, |view, _| view.preview_reload());
        })
    };
    let browser = {
        let this = this.clone();
        icon_button(
            "screen-preview-browser",
            menu_icons::OPEN_BROWSER,
            "Open in Browser",
            "Open the screen in the Browser tab",
        )
        .on_click(move |_, window, cx| {
            let _ = this.update(cx, |view, cx| view.preview_in_browser(window, cx));
        })
    };
    let zooms: Vec<AnyElement> = match tight {
        // One toggle that says the zoom shown and switches to the other.
        true => {
            let this = this.clone();
            let actual = preview.zoom == Zoom::Actual;
            vec![
                Button::new("screen-preview-zoom")
                    .small()
                    .ghost()
                    .label(if actual { "100%" } else { "Fit" })
                    .tooltip(if actual {
                        "Fit the page in the window"
                    } else {
                        "Show the page at 100%"
                    })
                    .disabled(!actual && !fits)
                    .on_click(move |_, _, cx| {
                        let to = if actual { Zoom::Fit } else { Zoom::Actual };
                        let _ =
                            this.update(cx, |view, cx| view.preview_change(|p| p.zoom = to, cx));
                    })
                    .into_any_element(),
            ]
        }
        false => vec![
            zoom("screen-preview-fit", Zoom::Fit, "Fit").into_any_element(),
            zoom("screen-preview-actual", Zoom::Actual, "100%").into_any_element(),
        ],
    };
    // Back, once a click in the page walked to another screen ([`super::prototype`]).
    let back = (!preview.back.is_empty()).then(|| {
        let this = this.clone();
        let mut back = Button::new("screen-preview-back")
            .small()
            .ghost()
            .icon(Lucide::ChevronLeft)
            .accessibility_label("Back to the Previous Screen")
            .tooltip("Back to the Previous Screen")
            .on_click(move |_, _, cx| {
                let _ = this.update(cx, |view, cx| view.preview_back(cx));
            });
        if !tight {
            back = back.label("Back");
        }
        back
    });
    let mut close = Button::new("screen-preview-close")
        .small()
        .icon(menu_icons::CLOSE)
        .accessibility_label("Close Preview")
        .tooltip("Close Preview (Escape)")
        .on_click(move |_, _, cx| {
            let _ = this.update(cx, |view, cx| view.close_preview(cx));
        });
    if !tight {
        close = close.label("Close");
    }
    let title = {
        let title = SharedString::from(preview.title.clone());
        let tip = title.clone();
        div()
            .id("screen-preview-title")
            .flex_1()
            .min_w_0()
            .truncate()
            .font_semibold()
            .tooltip(move |window, cx| Tooltip::new(tip.clone()).build(window, cx))
            .child(title)
    };
    let error = (preview.custom.as_ref()).and_then(|e| e.error.clone());
    let editing = preview.custom.is_some();
    let rule = |cx: &App| (!tight).then(|| divider(cx));
    let mut bar = h_flex()
        .h(px(BAR))
        .flex_shrink_0()
        .px_3()
        .gap_1()
        .items_center()
        .border_b_1()
        .border_color(theme.border)
        .bg(theme.background)
        .children(back)
        .child(title)
        .child(div().flex_shrink_0().child(size_part));
    // While a custom size is typed, the bar holds its fields, then Close.
    if !editing {
        bar = bar
            .child(rotate)
            .children(rule(cx))
            .children(zooms)
            .child(framed)
            .children(rule(cx))
            .child(reload)
            .child(browser);
    }
    let bar = bar.children(rule(cx)).child(close);
    // The bezel and its shadow, behind the page where it was last placed.
    let bezel = (preview.frame_shown())
        .then_some(preview.placed)
        .flatten()
        .map(|(f, _)| {
            div()
                .absolute()
                .left(px(f.x - BEZEL))
                .top(px(f.y - BEZEL))
                .w(px(f.width + 2.0 * BEZEL))
                .h(px(f.height + 2.0 * BEZEL))
                .rounded(px(BEZEL_RADIUS))
                .bg(rgb(0x0e1013))
                .border_1()
                .border_color(rgb(0x3b414d))
                .shadow_lg()
        });
    // What to type instead, under the fields, in the room the page leaves for it.
    let note = error.map(|error| {
        h_flex()
            .absolute()
            .top(px(BAR + 8.))
            .left(px(12.))
            .h(px(20.))
            .gap_1p5()
            .items_center()
            .text_xs()
            .text_color(theme.foreground)
            .child(
                Icon::new(Lucide::CircleAlert)
                    .xsmall()
                    .text_color(theme.danger),
            )
            .child(error)
    });
    Some(
        div()
            .id("screen-preview")
            .debug_selector(|| "screen-preview".into())
            .occlude()
            .absolute()
            .inset_0()
            .bg(theme.muted)
            .children(bezel)
            .child(v_flex().size_full().child(bar).child(div().flex_1()))
            .children(note),
    )
}
