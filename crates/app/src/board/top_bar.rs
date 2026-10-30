//! The canvas's top bar, a strip of [`HEIGHT`] over the canvas (spec D11, "Toolbar and context
//! bar"): the document's name and the zoom menu on the left, Hide UI and Maximize on the right.
//! Plan 6 puts the main menu first, and Plans 4 and 5 add their panels' toggles after the zoom
//! menu (ruling P3-C11).
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
use sawit_board::menu::shortcut_label;
use sawit_core::nyawit;
use serde_json::json;

use super::BoardView;
use crate::canvas::{TITLE, ToggleCanvasMaximize};

/// The bar's height.
pub(super) const HEIGHT: f32 = 36.;

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

/// The top bar of `view`'s canvas.
pub(super) fn render_top_bar(
    view: &mut BoardView,
    _: &mut Window,
    cx: &mut Context<BoardView>,
) -> impl IntoElement + use<> {
    let this = cx.entity().downgrade();
    let zoom = (view.board())
        .and_then(|board| board.session.ds().ok())
        .map_or(1.0, |ds| ds.view.zoom);
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
                .child(
                    div()
                        .min_w_0()
                        .px_1()
                        .text_sm()
                        .font_medium()
                        .truncate()
                        .child(document_name(view.path())),
                )
                .child(zoom_menu(zoom, this.clone())),
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
                        .tooltip(with_keys("Hide UI", shortcut_label("view.toggleUi")))
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
                        // The workspace's key for it, which the canvas leaves to Sawit unless it
                        // edits text (spec, "Keyboard conflict policy").
                        .tooltip("Maximize  ⇧⌘↩")
                        .debug_selector(|| "board-maximize".into())
                        .on_click(move |_, _, cx| {
                            let _ = this.update(cx, |_, cx| cx.emit(ToggleCanvasMaximize));
                        }),
                ),
        )
}

/// What the bar calls the document in `path`: "Canvas" for the canvas a project or the app has
/// (`canvas.figcraft`, the file the store takes for it), else the file's name.
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

/// `label` with the keys that run it after two spaces, as the chrome's tooltips read them
/// (`Hide UI  ⌘\`); `label` alone when it has none.
pub(super) fn with_keys(label: &str, keys: Option<String>) -> String {
    match keys {
        Some(keys) => format!("{label}  {keys}"),
        None => label.to_owned(),
    }
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
        .dropdown_menu(move |mut menu: PopupMenu, _, _| {
            for (i, group) in ZOOMS.iter().enumerate() {
                if i > 0 {
                    menu = menu.separator();
                }
                for &command in group.iter() {
                    menu = menu.item(zoom_item(command, view.clone()));
                }
            }
            menu
        })
}

/// The zoom menu's item for `command`: the engine's label for it, and its keys on the right.
fn zoom_item(command: &'static str, view: WeakEntity<BoardView>) -> PopupMenuItem {
    let label = find_command(command).map_or(command, |spec| spec.label);
    let keys = shortcut_label(command);
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
    .on_click(move |_, _, cx| {
        let _ = view.update(cx, |view, cx| view.choose(command, json!({}), cx));
    })
}
