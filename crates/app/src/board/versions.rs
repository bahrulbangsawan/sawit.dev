//! Version history (File ▸ Show version history, `file.versionHistory`): the versions the file
//! keeps, newest first, each its name, when it was saved and whether it was an automatic one,
//! with Restore (one undo step; the design as it was is kept in the history first) and Delete.
//! File ▸ Save to version history adds one (`sawit_board::prompts`).

use gpui_kit::component::button::*;
use gpui_kit::component::*;
use gpui_kit::*;
use serde_json::{Value, json};

use super::BoardView;

/// The window's versions, as `version.list` gives them, newest first.
pub(super) struct VersionsUi {
    list: Vec<Value>,
}

impl BoardView {
    /// Opens the window with the versions as they are now, or closes it.
    pub(super) fn toggle_versions(&mut self, cx: &mut Context<Self>) {
        if self.versions_ui.take().is_some() {
            cx.notify();
            return;
        }
        self.refresh_versions(cx);
    }

    fn refresh_versions(&mut self, cx: &mut Context<Self>) {
        let Some(board) = self.board_mut() else {
            return;
        };
        let mut list = board
            .execute("version.list", &json!({}))
            .ok()
            .and_then(|v| v.as_array().cloned())
            .unwrap_or_default();
        list.reverse();
        self.versions_ui = Some(VersionsUi { list });
        cx.notify();
    }

    fn version_action(&mut self, command: &'static str, id: Value, cx: &mut Context<Self>) {
        self.choose(command, json!({"id": id}), cx);
        // The history isn't the design: written now, as a saved version is.
        self.saved = None;
        if let Err(why) = self.save_now() {
            self.show_toast(why, cx);
        }
        self.refresh_versions(cx);
    }
}

fn when(seconds: u64) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let ago = now.saturating_sub(seconds);
    match ago {
        0..60 => "just now".to_string(),
        60..3600 => format!("{} min ago", ago / 60),
        3600..86_400 => format!("{} h ago", ago / 3600),
        _ => format!("{} days ago", ago / 86_400),
    }
}

/// The window, while it is open.
pub(super) fn render_versions(
    view: &mut BoardView,
    cx: &mut Context<BoardView>,
) -> Option<AnyElement> {
    let ui = view.versions_ui.as_ref()?;
    let theme = cx.theme().clone();
    let mut list = v_flex()
        .id("versions-list")
        .gap_1()
        .max_h(px(420.))
        .overflow_y_scroll();
    if ui.list.is_empty() {
        list = list.child(
            div()
                .text_sm()
                .text_color(theme.muted_foreground)
                .child("No versions yet. File ▸ Save to version history keeps one."),
        );
    }
    for (n, version) in ui.list.iter().enumerate() {
        let id = version.get("id").cloned().unwrap_or(Value::Null);
        let name = version
            .get("name")
            .and_then(Value::as_str)
            .filter(|n| !n.is_empty())
            .unwrap_or("Untitled version")
            .to_string();
        let auto = version
            .get("auto")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let time = version
            .get("time")
            .and_then(Value::as_u64)
            .map(when)
            .unwrap_or_default();
        let (restore_id, delete_id) = (id.clone(), id);
        list =
            list.child(
                h_flex()
                    .gap_2()
                    .p_2()
                    .items_center()
                    .rounded(px(6.))
                    .border_1()
                    .border_color(theme.border)
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .child(div().truncate().font_weight(FontWeight::MEDIUM).child(name))
                            .child(div().text_xs().text_color(theme.muted_foreground).child(
                                if auto {
                                    format!("Autosaved · {time}")
                                } else {
                                    time
                                },
                            )),
                    )
                    .child(
                        Button::new(("version-restore", n))
                            .ghost()
                            .xsmall()
                            .label("Restore")
                            .on_click(cx.listener(move |view, _, _, cx| {
                                view.version_action("version.restore", restore_id.clone(), cx)
                            })),
                    )
                    .child(
                        Button::new(("version-delete", n))
                            .ghost()
                            .xsmall()
                            .label("Delete")
                            .on_click(cx.listener(move |view, _, _, cx| {
                                view.version_action("version.delete", delete_id.clone(), cx)
                            })),
                    ),
            );
    }
    Some(
        v_flex()
            .id("versions-backdrop")
            .occlude()
            .absolute()
            .inset_0()
            .items_center()
            .pt(px(64.))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|view, _, _, cx| view.toggle_versions(cx)),
            )
            .child(
                v_flex()
                    .id("versions-window")
                    .debug_selector(|| "versions-window".into())
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .w(px(440.))
                    .max_w_full()
                    .p_4()
                    .gap_3()
                    .rounded(theme.radius_lg)
                    .border_1()
                    .border_color(theme.border)
                    .bg(theme.popover)
                    .text_color(theme.popover_foreground)
                    .shadow_lg()
                    .child(
                        h_flex()
                            .justify_between()
                            .child(
                                div()
                                    .text_base()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child("Version history"),
                            )
                            .child(
                                Button::new("versions-close")
                                    .ghost()
                                    .small()
                                    .label("Close")
                                    .on_click(
                                        cx.listener(|view, _, _, cx| view.toggle_versions(cx)),
                                    ),
                            ),
                    )
                    .child(list),
            )
            .into_any_element(),
    )
}
