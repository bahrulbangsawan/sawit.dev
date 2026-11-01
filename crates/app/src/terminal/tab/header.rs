//! A pane's header (what runs there, where, and its buttons) and the banner of a pane whose
//! process has ended.

use std::path::{Path, PathBuf};

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::*;
use gpui_kit::component::{ActiveTheme as _, Icon, Sizable as _, h_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use sawit_core::terminal::PaneId;

use super::look::PaneStyle;
use super::{HEADER_H, TabEvent, TerminalTab};
use crate::states::Banner;
use crate::terminal::view::TerminalView;
use crate::terminal::{SplitRight, agents};

/// The line under a pane's header.
const HEADER_BORDER: Pixels = px(1.);

impl TerminalTab {
    /// A pane's header: what runs there and where (not for SSH, whose
    /// folder would be the local `ssh`'s), whether it is zoomed, and its
    /// buttons. A long name or folder is cut short before the buttons are.
    /// The focused pane's header takes the active tab's colours and a heavier
    /// title ([`pane_style`]). While a coding agent runs in the pane, its logo
    /// stands in for the glyph, named for assistive technology and in a
    /// tooltip; the glyph comes back when the agent exits.
    ///
    /// [`pane_style`]: super::look::pane_style
    pub(super) fn render_header(
        &self,
        pane: PaneId,
        view: &Entity<TerminalView>,
        style: PaneStyle,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let remote = view.read(cx).remote.is_some();
        let folder = if remote {
            None
        } else {
            view.update(cx, |v, cx| v.folder(cx)).map(|dir| tilde(&dir))
        };
        let v = view.read(cx);
        let glyph = if remote {
            Lucide::Server
        } else if v.job.is_some() {
            Lucide::Play
        } else {
            Lucide::Terminal
        };
        let icon = match v.agent() {
            Some(agent) => {
                let running = agents::running_phrase(&[agent]);
                let tip = running.clone();
                div()
                    .id(("pane-agent", pane))
                    .debug_selector(move || format!("pane-agent-{pane}-{}", agent.id()))
                    .flex_none()
                    .role(Role::Image)
                    .aria_label(running)
                    .tooltip(move |window, cx| agents::tooltip(vec![tip.clone()], window, cx))
                    .child(agents::logo(agent, agents::LOGO, window, cx))
                    .into_any_element()
            }
            None => div()
                .flex_none()
                .child(Icon::new(glyph).xsmall())
                .into_any_element(),
        };
        let title = self.pane_label(pane, v);
        let zoomed = self.state.zoomed == Some(pane);
        let (zoom_icon, zoom_tip) = if zoomed {
            (Lucide::Minimize, "Unzoom")
        } else {
            (Lucide::Maximize, "Zoom")
        };
        let muted = cx.theme().muted_foreground;
        h_flex()
            .h(HEADER_H)
            .flex_none()
            .pl_2()
            .pr_1()
            .gap_1()
            .text_xs()
            .bg(style.header_bg)
            .text_color(style.title)
            .border_b(HEADER_BORDER)
            .border_color(cx.theme().border)
            .child(icon)
            .child(
                self.rename_field(cx.entity().downgrade(), pane)
                    .unwrap_or_else(|| {
                        div()
                            .min_w_0()
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .text_ellipsis()
                            .font_weight(style.title_weight)
                            .child(title)
                            .into_any_element()
                    }),
            )
            .when(zoomed, |el| {
                el.child(div().flex_none().text_color(muted).child("Zoomed"))
            })
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_ellipsis()
                    .text_color(muted)
                    .when_some(folder, |el, folder| el.child(folder)),
            )
            .child(
                h_flex()
                    .flex_none()
                    .gap_1()
                    .child(
                        Button::new(("pane-split", pane))
                            .ghost()
                            .xsmall()
                            .icon(Lucide::Columns2)
                            .accessibility_label("Split Right")
                            .tooltip("Split right")
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.focus_pane(pane, window, cx);
                                window.dispatch_action(Box::new(SplitRight), cx);
                            })),
                    )
                    .child(
                        Button::new(("pane-zoom", pane))
                            .ghost()
                            .xsmall()
                            .icon(zoom_icon)
                            .accessibility_label(zoom_tip)
                            .tooltip(zoom_tip)
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.toggle_zoom(pane, window, cx)
                            })),
                    )
                    .child(
                        Button::new(("pane-close", pane))
                            .ghost()
                            .xsmall()
                            .icon(Lucide::X)
                            .accessibility_label("Close Pane")
                            .tooltip("Close pane")
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.close_pane(pane, window, cx)
                            })),
                    ),
            )
    }

    /// What ended in `pane`, once its process has: an SSH session can
    /// reconnect, anything else can be replaced by a new shell tab. Either
    /// can close the pane (the tab, when it is the last).
    pub(super) fn render_banner(
        &self,
        pane: PaneId,
        view: &Entity<TerminalView>,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let v = view.read(cx);
        if !v.exited {
            return None;
        }
        let close_label = if self.state.is_split() {
            "Close Pane"
        } else {
            "Close Tab"
        };
        let banner = match &v.remote {
            Some(remote) => Banner::info(
                ("ssh-ended", pane),
                format!("Disconnected from {}.", remote.name),
            )
            .action(
                Button::new(("ssh-reconnect", pane))
                    .ghost()
                    .xsmall()
                    .icon(Lucide::RefreshCw)
                    .label("Reconnect")
                    .on_click(cx.listener(move |_, _, _, cx| cx.emit(TabEvent::Reconnect(pane)))),
            )
            .action(
                Button::new(("ssh-close", pane))
                    .ghost()
                    .xsmall()
                    .label(close_label)
                    .on_click(
                        cx.listener(move |this, _, window, cx| this.close_pane(pane, window, cx)),
                    ),
            ),
            None => Banner::info(("term-exited", pane), "This process has exited.")
                .action(
                    Button::new(("exited-close", pane))
                        .ghost()
                        .xsmall()
                        .label(close_label)
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.close_pane(pane, window, cx)
                        })),
                )
                .action(
                    Button::new(("exited-new", pane))
                        .ghost()
                        .xsmall()
                        .label("New Terminal")
                        .on_click(cx.listener(|_, _, _, cx| cx.emit(TabEvent::NewTerminal))),
                ),
        };
        Some(banner.into_any_element())
    }
}

/// `~/…` for folders in the home folder.
fn tilde(path: &Path) -> String {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    match home
        .as_deref()
        .and_then(|home| path.strip_prefix(home).ok())
    {
        Some(rest) if rest.as_os_str().is_empty() => "~".into(),
        Some(rest) => format!("~/{}", rest.display()),
        None => path.display().to_string(),
    }
}
