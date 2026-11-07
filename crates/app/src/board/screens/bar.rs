//! The bar over a selected screen, in the context bar's place, on the canvas's one floating bar
//! ([`FloatBar`], B13): Preview | Export ▾ (HTML, React, PNG) | Send to Agent ▾, the bar's one
//! primary action: a menu of the coding agents on this Mac that Sawit can start with a prompt,
//! each with its logo, then Copy Prompt and Reveal in Finder. The downloads ask where to save,
//! starting in the project's folder. The artboard and flow bars ([`super::frame_bar`]) share the
//! Export and Send to Agent menus.

use figcraft_doc::NodeId;
use gpui_kit::component::button::*;
use gpui_kit::component::menu::{MENU_ICON_SIZE, MENU_ICON_SLOT, PopupMenu, PopupMenuItem};
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::*;
use gpui_kit::*;
use sawit_board::frame_code;
use sawit_board::screens::{self, export};
use sawit_core::terminal::AgentKind;

use super::handoff::SendTo;
use crate::board::BoardView;
use crate::board::float_bar::{self, BarButton, FloatBar};
use crate::board::toolbar;
use crate::menu_icons;

/// What Send to Agent's menu lists while the agents are looked for, and when there are none.
pub(in crate::board) const LOOKING: &str = "Looking for coding agents…";
pub(in crate::board) const NONE_FOUND: &str = "No coding agent found";
/// The header over the agents in Send to Agent's menu.
pub(in crate::board) const AGENTS_HEADER: &str = "Build in this project";

/// The bar over the screen selected alone, when one is and the canvas isn't editing text.
pub(in crate::board) fn render_screen_bar(
    view: &mut BoardView,
    window: &mut Window,
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
    let top_centre = ((x0 + x1) / 2.0, y0 - float_bar::ABOVE - float_bar::HEIGHT);
    view.find_agents(cx);
    let agents = view.screen_ui.agents.clone();
    let frame = selected.frame;
    let folder = view.handoff_folder(&[frame]);
    let this = cx.entity().downgrade();
    let preview = {
        let this = this.clone();
        BarButton::new("screen-bar-preview", menu_icons::PREVIEW, "Preview")
            .tooltip("Preview the screen full size, live")
            .on_click(move |_, window, cx| {
                let _ = this.update(cx, |view, cx| view.open_preview(frame, window, cx));
            })
    };
    let bar = FloatBar::new("screen-bar")
        .button(preview)
        .divider()
        .button(export_button(
            this.clone(),
            "screen-bar-export",
            frame,
            true,
        ))
        .divider()
        .button(send_button(
            "screen-bar-send",
            "Send to Agent",
            vec![frame],
            agents,
            folder,
            this,
        ));
    Some(bar.render(top_centre, canvas, window, cx))
}

/// Export ▾ of `frame` (a screen when `screen`, else an artboard): HTML and React, which ask
/// where and write it ([`BoardView::export_code`]), then PNG (a screen's picture, asking where;
/// an artboard to the canvas's exports folder) and, for an artboard, SVG.
pub(in crate::board) fn export_button(
    this: WeakEntity<BoardView>,
    id: &'static str,
    frame: NodeId,
    screen: bool,
) -> BarButton {
    BarButton::new(id, menu_icons::EXPORT, "Export")
        .tooltip("Save the design as code or a picture")
        .menu(move |menu, _, _| {
            let code = |react: bool| {
                let this = this.clone();
                move |_: &ClickEvent, window: &mut Window, cx: &mut App| {
                    let _ = this.update(cx, |view, cx| view.export_code(frame, react, window, cx));
                }
            };
            let picture = |format: &'static str| {
                let this = this.clone();
                move |_: &ClickEvent, window: &mut Window, cx: &mut App| {
                    let _ = this.update(cx, |view, cx| match (screen, format) {
                        (true, _) => view.export_screen_png(frame, window, cx),
                        (false, "svg") => view.run_menu("file.exportSvg", window, cx),
                        (false, _) => view.run_menu("file.export", window, cx),
                    });
                }
            };
            let menu = menu
                .item(
                    PopupMenuItem::new("HTML")
                        .icon(menu_icons::HTML)
                        .tooltip("One HTML file with its CSS and images inlined")
                        .on_click(code(false)),
                )
                .item(
                    PopupMenuItem::new("React")
                        .icon(menu_icons::REACT)
                        .tooltip("A React component and its CSS: a reference to rebuild from")
                        .on_click(code(true)),
                )
                .separator()
                .item(
                    PopupMenuItem::new("PNG")
                        .icon(menu_icons::IMAGE)
                        .on_click(picture("png")),
                );
            match screen {
                true => menu,
                false => menu.item(
                    PopupMenuItem::new("SVG")
                        .icon(menu_icons::VECTOR)
                        .on_click(picture("svg")),
                ),
            }
        })
}

/// Export HTML or Export React of `frame` (a screen or an artboard) as a button of its own, for
/// the Design panel's Export section: asks where, then writes it ([`BoardView::export_code`]).
pub(in crate::board) fn code_export_button(
    this: WeakEntity<BoardView>,
    id: &'static str,
    frame: NodeId,
    react: bool,
) -> Button {
    let (label, icon, tip) = match react {
        true => (
            "Export React",
            menu_icons::REACT,
            "Save a React component and its CSS: a reference to rebuild from",
        ),
        false => (
            "Export HTML",
            menu_icons::HTML,
            "Save one HTML file with its CSS and images inlined",
        ),
    };
    Button::new(id)
        .small()
        .icon(icon)
        .label(label)
        .tooltip(tip)
        .on_click(move |_, window, cx| {
            let _ = this.update(cx, |view, cx| view.export_code(frame, react, window, cx));
        })
}

/// Send to Agent ▾ (`label`) for `frames`, the bar's primary action: its menu is
/// [`agents_menu`].
pub(in crate::board) fn send_button(
    id: &'static str,
    label: &'static str,
    frames: Vec<NodeId>,
    agents: Option<Vec<AgentKind>>,
    folder: Option<String>,
    this: WeakEntity<BoardView>,
) -> BarButton {
    BarButton::new(id, menu_icons::AGENT, label)
        .primary()
        .tooltip("Hand the design to a coding agent to build in this project")
        .menu(move |menu, _, _| agents_menu(menu, &frames, &agents, folder.clone(), &this))
}

/// Send to Agent's menu for `frames` (one, or a flow): a header and each coding agent found, with
/// its logo; while they are looked for, a spinner; with none, Get a Coding Agent…, which opens
/// the Dev Tools page. Copy Prompt and Reveal in Finder (the handoff `folder` in its tooltip)
/// always.
pub(in crate::board) fn agents_menu(
    mut menu: PopupMenu,
    frames: &[NodeId],
    agents: &Option<Vec<AgentKind>>,
    folder: Option<String>,
    view: &WeakEntity<BoardView>,
) -> PopupMenu {
    let send = |to: SendTo| {
        let (frames, view) = (frames.to_vec(), view.clone());
        move |_: &ClickEvent, window: &mut Window, cx: &mut App| {
            let _ = view.update(cx, |view, cx| {
                view.send_frames(frames.clone(), to, window, cx)
            });
        }
    };
    match agents {
        None => {
            menu = menu.item(
                PopupMenuItem::new(LOOKING)
                    .leading(|_, _| Spinner::new().with_size(MENU_ICON_SIZE))
                    .disabled(true),
            );
        }
        Some(agents) if agents.is_empty() => {
            menu = menu
                .item(
                    PopupMenuItem::new(NONE_FOUND)
                        .icon(menu_icons::AGENT)
                        .disabled(true),
                )
                .item(
                    PopupMenuItem::new("Get a Coding Agent…")
                        .icon(menu_icons::INSTALL)
                        .on_click(|_, window, cx| {
                            window.dispatch_action(Box::new(crate::workspace::OpenDevTools), cx)
                        }),
                );
        }
        Some(agents) => {
            menu = menu.label(AGENTS_HEADER);
            for kind in agents.iter().copied() {
                let item = PopupMenuItem::new(kind.name())
                    .leading(move |window, cx| {
                        let tint = cx.theme().popover_foreground;
                        crate::terminal::agent_logo(kind, MENU_ICON_SLOT, tint, window, cx)
                    })
                    .on_click(send(SendTo::Agent(kind)));
                menu = menu.item(item);
            }
        }
    }
    let reveal = PopupMenuItem::new("Reveal in Finder")
        .icon(menu_icons::REVEAL)
        .on_click(send(SendTo::Reveal));
    let reveal = match folder {
        Some(folder) => reveal.tooltip(folder),
        None => reveal,
    };
    menu.separator()
        .item(
            PopupMenuItem::new("Copy Prompt")
                .icon(menu_icons::COPY)
                .on_click(send(SendTo::CopyPrompt)),
        )
        .item(reveal)
}

impl BoardView {
    /// The folder Send to Agent writes `frames`' handoff to, as the Finder shows it (`~/…`).
    pub(in crate::board) fn handoff_folder(&self, frames: &[NodeId]) -> Option<String> {
        let ds = self.board()?.session.ds().ok()?;
        let targets = sawit_board::handoff::targets(&ds.doc, frames).ok()?;
        let name = sawit_board::handoff::name(&targets);
        let folder = sawit_board::handoff::folder(&self.screens_dir(), &name);
        let shown = folder.display().to_string();
        Some(match std::env::var("HOME") {
            Ok(home) if !home.is_empty() && shown.starts_with(&home) => {
                format!("~{}", &shown[home.len()..])
            }
            _ => shown,
        })
    }

    /// Export ▸ PNG of screen `frame`: its latest picture, made a PNG off the UI thread, saved
    /// where the user picks, starting in the project's folder with the screen's name.
    pub(in crate::board) fn export_screen_png(
        &mut self,
        frame: NodeId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(picture) = self.screen_picture(frame) else {
            self.show_toast("The screen hasn't drawn yet: try again once it shows", cx);
            return;
        };
        let name = (self.board().and_then(|b| b.session.ds().ok()))
            .and_then(|ds| {
                ds.doc
                    .node(frame)
                    .and_then(screens::screen_of)
                    .map(|s| s.screen)
            })
            .unwrap_or_else(|| "screen".to_string());
        let start = self.canvas_project().unwrap_or_else(|| {
            std::env::var_os("HOME").map_or_else(|| "/".into(), std::path::PathBuf::from)
        });
        let picked = cx.prompt_for_new_path(&start, Some(&format!("{name}.png")));
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(path))) = picked.await else {
                return;
            };
            let written = cx
                .background_spawn(async move {
                    let png = super::pictures::png_of(&picture).ok_or("the picture is empty")?;
                    std::fs::write(&path, png).map_err(|e| e.to_string())?;
                    Ok::<_, String>(path)
                })
                .await;
            let said = match written {
                Ok(path) => format!(
                    "Saved {}",
                    path.file_name()
                        .map(|n| n.to_string_lossy())
                        .unwrap_or_default()
                ),
                Err(error) => format!("Couldn't save it: {error}"),
            };
            let _ = this.update(cx, |this, cx| this.show_toast(said, cx));
        })
        .detach();
    }

    /// Looks for the coding agents on this Mac once, off the UI thread.
    pub(in crate::board) fn find_agents(&mut self, cx: &mut Context<Self>) {
        // Without the runtime (a canvas in a test) there is nothing to look with.
        if !cx.has_global::<crate::rt::Rt>() {
            return;
        }
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

    /// Export HTML or Export React of frame `frame`, a screen or an artboard: asks where,
    /// starting in the project's folder with the frame's name, then writes it there (a screen's
    /// off the UI thread, from its folder; an artboard's from the document).
    pub(in crate::board) fn export_code(
        &mut self,
        frame: NodeId,
        react: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(ds) = self.board().and_then(|b| b.session.ds().ok()) else {
            return;
        };
        let Some(node) = ds.doc.node(frame) else {
            return;
        };
        let screen = screens::screen_of(node).map(|s| s.screen);
        let file = match &screen {
            Some(name) if react => format!("{}.tsx", screens::component_name(name)),
            Some(name) => format!("{name}.html"),
            None => frame_code::file_name(&node.name, react),
        };
        let start = self.canvas_project().unwrap_or_else(|| {
            std::env::var_os("HOME").map_or_else(|| "/".into(), std::path::PathBuf::from)
        });
        let folder = screen
            .as_ref()
            .map(|name| screens::folder(&self.screens_dir(), name));
        let picked = cx.prompt_for_new_path(&start, Some(&file));
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(path))) = picked.await else {
                return;
            };
            let written = match (screen, folder) {
                (Some(name), Some(folder)) => {
                    cx.background_spawn(async move {
                        match react {
                            true => export::write_react(&folder, &name, &path),
                            false => export::write_html(&folder, &path),
                        }
                    })
                    .await
                }
                _ => {
                    let made = this.update(cx, |this, _| {
                        let board = this.board_mut().ok_or("the canvas isn't open")?;
                        match react {
                            true => frame_code::write_react(board, frame, &path),
                            false => frame_code::write_html(board, frame, &path),
                        }
                    });
                    made.unwrap_or_else(|_| Err("the canvas closed".into()))
                }
            };
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
