//! The canvas's top bar, a strip of [`HEIGHT`] over the canvas (spec D11, "Toolbar and context
//! bar"): the main menu's ≡ button ([`super::menus`]), the document's name, the zoom menu, and
//! the layers panel's and the Design panel's toggles on the left (rulings P3-C11, P4-C13), Hide
//! UI and Maximize on the right.
//!
//! Like the rest of the chrome, the bar goes while the UI is hidden (`⌘\`, `view.toggleUi`), so
//! its Hide UI button only ever hides: `⌘\` shows the UI again (ruling P3-C12).

use std::path::Path;

use figcraft_engine::cmd::find_command;
use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::*;
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenu, PopupMenuItem};
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use sawit_core::nyawit;
use serde_json::json;

use super::{BoardView, TITLE, ToggleCanvasMaximize, command_keys, menus, option_icons};

/// The bar's height.
pub(super) const HEIGHT: f32 = 36.;

/// The picture of the main menu's button: ≡.
pub(super) const MAIN_MENU_ICON: Lucide = Lucide::Menu;

/// The zoom menu's commands, in the spec's order, in groups between separators.
const ZOOMS: [&[&str]; 3] = [
    &["view.zoomIn", "view.zoomOut"],
    &["view.zoomFit", "view.zoomSelection"],
    &["view.zoom50", "view.zoom100", "view.zoom200"],
];

/// The picture of the Hide UI button.
pub(super) const HIDE_UI_ICON: Lucide = Lucide::EyeOff;

/// The picture of the Maximize button.
pub(super) const MAXIMIZE_ICON: Lucide = Lucide::Maximize;

/// The picture of the layers panel's toggle.
pub(super) const LAYERS_ICON: Lucide = Lucide::PanelLeft;

/// The picture of the Design panel's toggle.
pub(super) const DESIGN_ICON: Lucide = Lucide::PanelRight;

/// The top bar of `view`'s canvas.
pub(super) fn render_top_bar(
    view: &mut BoardView,
    window: &mut Window,
    cx: &mut Context<BoardView>,
) -> impl IntoElement + use<> {
    let this = cx.entity().downgrade();
    let zoom = (view.board())
        .and_then(|board| board.session.ds().ok())
        .map_or(1.0, |ds| ds.view.zoom);
    let hide_ui = with_keys("Hide UI", command_keys("view.toggleUi", cx));
    let layers = with_keys("Layers panel", command_keys("view.layers", cx));
    let design = with_keys("Design panel", command_keys("view.design", cx));
    let theme = cx.theme();
    h_flex()
        .id("board-top-bar")
        .debug_selector(|| "board-top-bar".into())
        .flex_shrink_0()
        .w_full()
        .h(px(HEIGHT))
        .px_2()
        .gap_2()
        .border_b_1()
        .border_color(theme.border)
        .bg(theme.background)
        .text_color(theme.foreground)
        .child(
            h_flex()
                .flex_1()
                .min_w_0()
                .gap_1()
                .child(main_menu(this.clone()))
                .child(
                    div()
                        .min_w_0()
                        .px_1()
                        .text_sm()
                        .font_medium()
                        .truncate()
                        .child(document_name(view.path())),
                )
                .child(zoom_menu(zoom, this.clone()))
                .child(layers_toggle(view.show_layers, layers, this.clone()))
                .child(design_toggle(view.show_design, design, this.clone())),
        )
        .child(
            h_flex()
                .flex_shrink_0()
                .gap_1()
                .child(
                    Button::new("board-hide-ui")
                        .ghost()
                        .small()
                        .icon(HIDE_UI_ICON)
                        .tooltip(hide_ui)
                        .debug_selector(|| "board-hide-ui".into())
                        .on_click({
                            let this = this.clone();
                            move |_, _, cx| {
                                let _ = this.update(cx, |view, cx| {
                                    view.choose("view.toggleUi", json!({}), cx);
                                });
                            }
                        }),
                )
                .child(
                    Button::new("board-maximize")
                        .ghost()
                        .small()
                        .icon(MAXIMIZE_ICON)
                        .tooltip(maximize_tooltip(window))
                        .debug_selector(|| "board-maximize".into())
                        .on_click(move |_, _, cx| {
                            let _ = this.update(cx, |_, cx| cx.emit(ToggleCanvasMaximize));
                        }),
                ),
        )
}

/// What the bar calls the document in `path`: "Canvas" for the canvas a project or the app has
/// (`canvas.sawit`, the file the store takes for it), else the file's name.
pub(super) fn document_name(path: &Path) -> SharedString {
    match path.file_name() {
        Some(name) if name == nyawit::BOARD_FILE => TITLE.into(),
        Some(name) => name.to_string_lossy().into_owned().into(),
        None => path.display().to_string().into(),
    }
}

/// The zoom as the zoom button shows it: `100%` at 1.
pub(super) fn zoom_label(zoom: f64) -> String {
    format!("{:.0}%", zoom * 100.0)
}

/// The Maximize button's tooltip, with the workspace's key for it as the keymap is now
/// (`Maximize  ⇧⌘↩`), which the canvas leaves to Sawit unless it edits text (spec, "Keyboard
/// conflict policy").
pub(super) fn maximize_tooltip(window: &Window) -> String {
    let maximize = &crate::workspace::ToggleMaximizePane;
    let keys = crate::keymap::keys_for(maximize, "Workspace", window);
    with_keys("Maximize", (!keys.is_empty()).then(|| keys.to_string()))
}

/// `label` with the keys that run it after two spaces, as the chrome's tooltips read them
/// (`Hide UI  ⌘\`); `label` alone when it has none.
pub(super) fn with_keys(label: &str, keys: Option<String>) -> String {
    match keys {
        Some(keys) => format!("{label}  {keys}"),
        None => label.to_owned(),
    }
}

/// The ≡ button, and its menu: the main menu (spec D8), as it is when it drops.
fn main_menu(view: WeakEntity<BoardView>) -> impl IntoElement + use<> {
    Button::new("board-main-menu")
        .ghost()
        .small()
        .icon(MAIN_MENU_ICON)
        .tooltip("Main menu")
        .debug_selector(|| "board-main-menu".into())
        .dropdown_menu(move |menu: PopupMenu, window, cx| {
            menus::fill_main_menu(menu, &view, window, cx)
        })
}

/// The zoom button, showing `zoom`, and its menu of the zoom commands, each with its keys.
fn zoom_menu(zoom: f64, view: WeakEntity<BoardView>) -> impl IntoElement + use<> {
    Button::new("board-zoom")
        .ghost()
        .small()
        .label(zoom_label(zoom))
        .dropdown_caret(true)
        .tooltip("Zoom")
        .debug_selector(|| "board-zoom".into())
        .dropdown_menu(move |mut menu: PopupMenu, _, cx| {
            for (i, group) in ZOOMS.iter().enumerate() {
                if i > 0 {
                    menu = menu.separator();
                }
                for &command in group.iter() {
                    let keys = command_keys(command, cx);
                    menu = menu.item(zoom_item(command, keys, view.clone()));
                }
            }
            menu
        })
}

/// The layers panel's toggle, marked while the panel is `shown`, with `tooltip`. It runs
/// `view.layers`, as ⌥1 does, and the canvas carries out the engine's `panel.layers` request.
fn layers_toggle(shown: bool, tooltip: String, view: WeakEntity<BoardView>) -> Button {
    Button::new("board-layers")
        .ghost()
        .small()
        .icon(LAYERS_ICON)
        .selected(shown)
        .tooltip(tooltip)
        .debug_selector(|| "board-layers".into())
        .on_click(move |_, _, cx| {
            let _ = view.update(cx, |view, cx| view.choose("view.layers", json!({}), cx));
        })
}

/// The Design panel's toggle, marked while the panel is `shown`, with `tooltip`. It runs
/// `view.design`, as ⌥8 does, and the canvas carries out the engine's `panel.design` request.
fn design_toggle(shown: bool, tooltip: String, view: WeakEntity<BoardView>) -> Button {
    Button::new("board-design")
        .ghost()
        .small()
        .icon(DESIGN_ICON)
        .selected(shown)
        .tooltip(tooltip)
        .debug_selector(|| "board-design".into())
        .on_click(move |_, _, cx| {
            let _ = view.update(cx, |view, cx| view.choose("view.design", json!({}), cx));
        })
}

/// The zoom menu's item for `command`: its picture, the engine's label for it, and its `keys` on
/// the right.
fn zoom_item(
    command: &'static str,
    keys: Option<String>,
    view: WeakEntity<BoardView>,
) -> PopupMenuItem {
    let label = find_command(command).map_or(command, |spec| spec.label);
    PopupMenuItem::element(move |_, cx| {
        h_flex()
            .debug_selector(|| format!("board-zoom-{command}"))
            .w_full()
            .gap_6()
            .justify_between()
            .child(label)
            .when_some(keys.clone(), |row, keys| {
                row.child(div().text_color(cx.theme().muted_foreground).child(keys))
            })
    })
    .when_some(option_icons::zoom(command), |item, picture| {
        item.icon(picture.icon())
    })
    .on_click(move |_, _, cx| {
        let _ = view.update(cx, |view, cx| view.choose(command, json!({}), cx));
    })
}
