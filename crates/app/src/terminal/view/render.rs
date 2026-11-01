//! The terminal's element: its input handlers, and the canvas that sizes the grid and paints.

use std::time::Instant;

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::{ActiveTheme as _, Icon, Sizable as _};
use gpui_kit::*;
use sawit_core::terminal::{GridSize, MouseAction, MouseButton as TermButton};

use super::mouse::{RightClick, pointer_moved_for_drop, right_click_target, to_term_button};
use super::paint::{FONT_SIZE, Painter, cell_size, hsla, paint_snapshot};
use super::{PAD_X, TerminalEvent, TerminalView};
use crate::files::DraggedFile;

impl Render for TerminalView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let font = font(cx.theme().mono_font_family.clone());
        let cell = cell_size(window, cx);
        let session = self.session.clone();
        let palette = self.palette.clone();
        let layout = self.layout.clone();
        let focused = self.focus.is_focused(window);
        if focused {
            self.keyboard_at = Some(Instant::now());
        }
        let exited = self.exited;
        let selection = cx.theme().selection;
        let this = cx.entity().downgrade();
        let toast = self.toast.is_some().then(|| {
            div()
                .absolute()
                .bottom_2()
                .right_3()
                .flex()
                .items_center()
                .gap_1()
                .px_2()
                .py_1()
                .rounded_md()
                .border_1()
                .border_color(cx.theme().border)
                .bg(cx.theme().popover)
                .text_color(cx.theme().popover_foreground)
                .text_xs()
                .shadow_md()
                .child(Icon::new(Lucide::Check).xsmall())
                .child("Copied to clipboard")
        });

        div()
            .id("terminal")
            .role(Role::Terminal)
            .aria_label("Terminal")
            .relative()
            .key_context("Terminal")
            .track_focus(&self.focus)
            .size_full()
            .px(PAD_X)
            .bg(hsla(self.palette.background))
            .on_key_down(cx.listener(Self::on_key))
            .on_scroll(cx.listener(Self::on_scroll))
            .on_drag_move(|e: &DragMoveEvent<ExternalPaths>, window, cx| {
                pointer_moved_for_drop(e.event.position, window, cx)
            })
            .on_drop(cx.listener(|this, paths: &ExternalPaths, window, cx| {
                this.drop_paths(paths.paths(), window, cx)
            }))
            .on_drop(cx.listener(|this, file: &DraggedFile, window, cx| {
                this.drop_paths(std::slice::from_ref(&file.path), window, cx)
            }))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, e: &MouseDownEvent, window, cx| {
                    this.focus(window, cx);
                    this.session.clear_selection();
                    // Shift takes the mouse back from programs that report it.
                    if !e.modifiers.shift
                        && this.send_mouse(MouseAction::Press(TermButton::Left), e.position, false)
                    {
                        this.pressed = Some(TermButton::Left);
                    } else {
                        this.start_selection(e.position, e.click_count);
                    }
                    cx.notify();
                }),
            )
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(|this, e: &MouseDownEvent, _, cx| {
                    let target = right_click_target(
                        this.send_right_clicks,
                        e.modifiers.shift,
                        this.modes().mouse_report,
                    );
                    match target {
                        RightClick::Menu => cx.emit(TerminalEvent::MenuRequested(e.position)),
                        RightClick::Program => {
                            if this.send_mouse(
                                MouseAction::Press(TermButton::Right),
                                e.position,
                                false,
                            ) {
                                this.pressed = Some(TermButton::Right);
                            }
                        }
                        RightClick::Nothing => {}
                    }
                }),
            )
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, e: &MouseUpEvent, _, _| {
                    if this.pressed.take().is_some() {
                        this.send_mouse(
                            MouseAction::Release(TermButton::Left),
                            e.position,
                            e.modifiers.shift,
                        );
                    }
                }),
            )
            .on_mouse_up(
                MouseButton::Right,
                cx.listener(|this, e: &MouseUpEvent, _, _| {
                    if this.pressed.take().is_some() {
                        this.send_mouse(
                            MouseAction::Release(TermButton::Right),
                            e.position,
                            e.modifiers.shift,
                        );
                    }
                }),
            )
            .on_mouse_move(cx.listener(|this, e: &MouseMoveEvent, _, _| {
                if this.selecting {
                    return;
                }
                let action = match e.pressed_button.and_then(to_term_button) {
                    Some(b) => MouseAction::Drag(b),
                    None => MouseAction::Move,
                };
                this.send_mouse(action, e.position, e.modifiers.shift);
            }))
            .child(
                canvas(
                    move |bounds, _, _| {
                        let cols = (bounds.size.width / cell.width).floor().max(2.) as u16;
                        let rows = (bounds.size.height / cell.height).floor().max(1.) as u16;
                        session.resize(GridSize {
                            cols,
                            rows,
                            cell_width: f32::from(cell.width) as u16,
                            cell_height: f32::from(cell.height) as u16,
                        });
                        layout.set(Some((bounds.origin, cell)));
                        session
                    },
                    move |bounds, session, window, cx| {
                        // Window-wide, so a drag that leaves the terminal
                        // keeps selecting and its release is not missed.
                        window.on_mouse_event({
                            let this = this.clone();
                            move |e: &MouseMoveEvent, phase, _, cx| {
                                if phase.bubble() && e.pressed_button == Some(MouseButton::Left) {
                                    let _ = this.update(cx, |this, cx| {
                                        this.extend_selection(e.position, cx)
                                    });
                                }
                            }
                        });
                        window.on_mouse_event(move |e: &MouseUpEvent, phase, _, cx| {
                            if phase.bubble() && e.button == MouseButton::Left {
                                let _ = this.update(cx, |this, cx| this.finish_selection(cx));
                            }
                        });
                        let snap = session.snapshot(&palette);
                        window.paint_quad(fill(bounds, hsla(palette.background)));
                        let painter = Painter {
                            palette: &palette,
                            origin: bounds.origin,
                            cell,
                            font: &font,
                            focused,
                            selection,
                        };
                        paint_snapshot(&snap, &painter, window, cx);
                        if exited {
                            let note = "[process exited]";
                            let run = TextRun {
                                len: note.len(),
                                font: font.clone(),
                                color: hsla(palette.ansi[8]),
                                background_color: None,
                                underline: None,
                                strikethrough: None,
                            };
                            let line = window.text_system().shape_line(
                                note.into(),
                                px(FONT_SIZE),
                                &[run],
                                None,
                            );
                            let y = bounds.origin.y + bounds.size.height - cell.height;
                            let _ = line.paint(
                                point(bounds.origin.x, y),
                                cell.height,
                                TextAlign::Left,
                                None,
                                window,
                                cx,
                            );
                        }
                    },
                )
                .size_full(),
            )
            .children(toast)
    }
}
