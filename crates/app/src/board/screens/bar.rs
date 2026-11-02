//! The bar over a selected screen, in the context bar's place: Preview, Download HTML, Download
//! React, and Send to Agent…, a menu of the coding agents on this Mac that Sawit can start with a
//! prompt, and Copy Prompt. The downloads ask where to save, starting in the project's folder.

use figcraft_doc::NodeId;
use gpui_kit::component::button::*;
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenu, PopupMenuItem};
use gpui_kit::component::*;
use gpui_kit::*;
use sawit_board::context_bar as model;
use sawit_board::screens::{self, export};
use sawit_core::terminal::AgentKind;

use crate::board::BoardView;
use crate::board::toolbar::{self, divider};

/// The bar's height and width, and the room between it and the screen's name above the frame.
const HEIGHT: f64 = 40.;
const WIDTH: f64 = 440.;
const ABOVE: f64 = 34.;

/// What Send to Agent's menu lists while the agents are looked for, and when there are none.
const LOOKING: &str = "Looking for coding agents…";
const NONE_FOUND: &str = "No coding agent Sawit can start was found";

/// The bar over the screen selected alone, when one is and the canvas isn't editing text.
pub(in crate::board) fn render_screen_bar(
    view: &mut BoardView,
    cx: &mut Context<BoardView>,
) -> Option<impl IntoElement + use<>> {
    if view.editing_text() || view.preview_open() {
        return None;
    }
    let selected = view.selected_screen()?;
    let ds = view.board()?.session.ds().ok()?;
    if view.board()?.session.tool.busy() {
        return None;
    }
    let b = ds.doc.world_bounds(selected.frame)?;
    let (x0, y0) = ds.view.page_to_screen(b.x0, b.y0);
    let (x1, _) = ds.view.page_to_screen(b.x1, b.y0);
    let under = f64::from(toolbar::BOTTOM + toolbar::HEIGHT);
    let canvas = (ds.view.vw, ds.view.vh - under);
    let width = WIDTH.min(canvas.0 - 2.0 * model::MARGIN).max(0.0);
    let top_centre = ((x0 + x1) / 2.0, y0 - ABOVE - HEIGHT);
    let (x, y) = model::clamp_anchor(top_centre, (width, HEIGHT), canvas);
    view.find_agents(cx);
    let agents = view.screen_ui.agents.clone();
    let this = cx.entity().downgrade();
    let frame = selected.frame;
    let theme = cx.theme();
    let line = theme.border;
    let preview = {
        let this = this.clone();
        Button::new("screen-bar-preview")
            .ghost()
            .small()
            .label("Preview")
            .tooltip("Preview the screen full size, live")
            .on_click(move |_, window, cx| {
                let _ = this.update(cx, |view, cx| view.open_preview(frame, window, cx));
            })
    };
    let download = |this: WeakEntity<BoardView>, id: &'static str, label: &'static str, react| {
        Button::new(id)
            .ghost()
            .small()
            .label(label)
            .on_click(move |_, window, cx| {
                let _ = this.update(cx, |view, cx| {
                    view.download_screen(frame, react, window, cx)
                });
            })
    };
    let to_agent = this.clone();
    let send = Button::new("screen-bar-send")
        .ghost()
        .small()
        .label("Send to Agent…")
        .dropdown_caret(true)
        .dropdown_menu(move |menu: PopupMenu, _, _| agents_menu(menu, frame, &agents, &to_agent));
    Some(
        h_flex()
            .id("screen-bar")
            .debug_selector(|| "screen-bar".into())
            .occlude()
            .absolute()
            .left(px((x - width / 2.0) as f32))
            .top(px(y as f32))
            .w(px(width as f32))
            .h(px(HEIGHT as f32))
            .px_1()
            .gap_0p5()
            .rounded(px(10.))
            .border_1()
            .border_color(theme.border)
            .bg(theme.popover)
            .text_color(theme.popover_foreground)
            .shadow_md()
            .overflow_x_scroll()
            .child(preview)
            .child(divider(line))
            .child(download(
                this.clone(),
                "screen-bar-html",
                "Download HTML",
                false,
            ))
            .child(download(
                this.clone(),
                "screen-bar-react",
                "Download React",
                true,
            ))
            .child(divider(line))
            .child(send),
    )
}

/// Send to Agent's menu: each coding agent found, then Copy Prompt.
fn agents_menu(
    mut menu: PopupMenu,
    frame: NodeId,
    agents: &Option<Vec<AgentKind>>,
    view: &WeakEntity<BoardView>,
) -> PopupMenu {
    match agents {
        None => menu = menu.item(PopupMenuItem::new(LOOKING).disabled(true)),
        Some(agents) if agents.is_empty() => {
            menu = menu.item(PopupMenuItem::new(NONE_FOUND).disabled(true));
        }
        Some(agents) => {
            menu = menu.label("Build it in this project with");
            for kind in agents {
                let (kind, view) = (*kind, view.clone());
                let item = PopupMenuItem::new(kind.name()).on_click(move |_, window, cx| {
                    let _ = view.update(cx, |view, cx| {
                        view.send_screen(frame, Some(kind), window, cx)
                    });
                });
                menu = menu.item(item);
            }
        }
    }
    let view = view.clone();
    menu.separator().item(
        PopupMenuItem::new("Copy Prompt").on_click(move |_, window, cx| {
            let _ = view.update(cx, |view, cx| view.send_screen(frame, None, window, cx));
        }),
    )
}

impl BoardView {
    /// Looks for the coding agents on this Mac once, off the UI thread.
    pub(in crate::board) fn find_agents(&mut self, cx: &mut Context<Self>) {
        let ui = &mut self.screen_ui;
        if ui.agents.is_some() || std::mem::replace(&mut ui.finding_agents, true) {
            return;
        }
        let found = crate::rt::spawn_blocking(cx, crate::browser::installed_agents);
        cx.spawn(async move |this, cx| {
            let agents = found.await.unwrap_or_default();
            let _ = this.update(cx, |this, cx| {
                this.screen_ui.agents = Some(agents);
                this.screen_ui.finding_agents = false;
                cx.notify();
            });
        })
        .detach();
    }

    /// Download HTML or Download React of screen frame `frame`: asks where, starting in the
    /// project's folder with the screen's name, then writes it there off the UI thread.
    pub(in crate::board) fn download_screen(
        &mut self,
        frame: NodeId,
        react: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(found) = self.selected_screen().filter(|s| s.frame == frame) else {
            return;
        };
        let name = found.data.screen.clone();
        let file = match react {
            true => format!("{}.tsx", screens::component_name(&name)),
            false => format!("{name}.html"),
        };
        let start = self.canvas_project().unwrap_or_else(|| {
            std::env::var_os("HOME").map_or_else(|| "/".into(), std::path::PathBuf::from)
        });
        let folder = screens::folder(&self.screens_dir(), &name);
        let picked = cx.prompt_for_new_path(&start, Some(&file));
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(path))) = picked.await else {
                return;
            };
            let written = cx
                .background_spawn(async move {
                    match react {
                        true => export::write_react(&folder, &name, &path),
                        false => export::write_html(&folder, &path),
                    }
                })
                .await;
            let said = match written {
                Ok(written) => {
                    let first = written.files.first();
                    let shown = first
                        .and_then(|p| p.file_name())
                        .map(|n| n.to_string_lossy());
                    format!("Saved {}", shown.unwrap_or_default())
                }
                Err(error) => format!("Couldn't save it: {error}"),
            };
            let _ = this.update(cx, |this, cx| this.show_toast(said, cx));
        })
        .detach();
    }
}
