//! Drawing the tab: one pane full size, or each pane of a split with its header and border,
//! the dividers between them, the pane menu, and the keys that act on panes.

use gpui_kit::base::POPUP_PRIORITY;
use gpui_kit::component::{ActiveTheme as _, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use sawit_core::terminal::{Direction, PaneId, Rect, SplitDir};

use super::look::{PaneLook, pane_style};
use super::panes::min_size;
use super::{Drag, PANE_BORDER, TerminalTab};
use crate::terminal::view::{TerminalView, cell_size};
use crate::terminal::{
    ClosePane, FocusDown, FocusLeft, FocusNextPane, FocusNextPaneAtPrompt, FocusPreviousPane,
    FocusRight, FocusUp, RenamePane, RenameTab, ToggleSendRightClicks, ZoomPane, agents,
};

/// Width of the grab strip centred on each divider.
const DIVIDER_HIT: f32 = 5.;

impl TerminalTab {
    /// One pane at `rect`, looking `look`: header, border, banner (once its
    /// process has ended) and terminal, dimmed when the pane is inactive. The
    /// dimming has no hitbox, so clicks and drops still reach the terminal.
    fn render_pane(
        &self,
        pane: PaneId,
        rect: Rect,
        view: &Entity<TerminalView>,
        look: PaneLook,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let v = view.read(cx);
        let style = pane_style(look, cx.theme(), v.background());
        let agent = v.agent();
        v_flex()
            .id(("pane", pane))
            .debug_selector(move || format!("pane-{pane}-{look:?}"))
            .role(Role::Group)
            .aria_label("Terminal pane")
            .when_some(agent, |el, agent| {
                el.aria_description(agents::running_phrase(&[agent]))
            })
            .aria_selected(pane == self.state.focused)
            .absolute()
            .left(px(rect.x))
            .top(px(rect.y))
            .w(px(rect.w))
            .h(px(rect.h))
            .overflow_hidden()
            .border(PANE_BORDER)
            .border_color(style.border)
            .on_any_mouse_down(
                cx.listener(move |this, _, window, cx| this.focus_pane(pane, window, cx)),
            )
            .child(self.render_header(pane, view, style, window, cx))
            .children(self.render_banner(pane, view, cx))
            .child(
                div()
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .child(view.clone())
                    .when_some(style.dim, |el, dim| {
                        el.child(div().absolute().inset_0().bg(dim))
                    }),
            )
    }

    /// Whether the keyboard is in the tab: in one of its panes, or in the field
    /// renaming one (which closes once it loses the keyboard).
    fn keyboard_here(&self, window: &Window, cx: &App) -> bool {
        self.renaming.is_some() || self.has_focus(window, cx)
    }

    /// How each pane looks now ([`PaneLook`]), in layout order (for tests).
    #[cfg(test)]
    pub(super) fn looks(&self, window: &Window, cx: &App) -> Vec<(PaneId, PaneLook)> {
        let here = self.keyboard_here(window, cx);
        self.state
            .layout
            .panes()
            .into_iter()
            .map(|pane| (pane, PaneLook::of(pane, self.state.focused, here)))
            .collect()
    }
}

impl Render for TerminalTab {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        #[cfg(test)]
        {
            self.renders += 1;
        }
        self.window = Some(window.window_handle());
        let bounds = self.bounds.clone();
        let this = cx.entity().downgrade();
        let menu = self.menu.as_ref().map(|menu| {
            deferred(
                anchored()
                    .position(menu.at)
                    .snap_to_window_with_margin(px(8.))
                    .child(menu.view.clone()),
            )
            .with_priority(POPUP_PRIORITY)
        });
        let root = div()
            .relative()
            .size_full()
            .children(menu)
            .on_action(cx.listener(|this, _: &FocusLeft, window, cx| {
                this.move_focus(Direction::Left, window, cx)
            }))
            .on_action(cx.listener(|this, _: &FocusRight, window, cx| {
                this.move_focus(Direction::Right, window, cx)
            }))
            .on_action(cx.listener(|this, _: &FocusUp, window, cx| {
                this.move_focus(Direction::Up, window, cx)
            }))
            .on_action(cx.listener(|this, _: &FocusDown, window, cx| {
                this.move_focus(Direction::Down, window, cx)
            }))
            .on_action(cx.listener(|this, _: &ZoomPane, _, cx| {
                // One pane: the workspace's binding for the key maximizes
                // the dock instead.
                if this.state.toggle_zoom() {
                    cx.notify();
                } else {
                    cx.propagate();
                }
            }))
            .on_action(
                cx.listener(|this, _: &FocusNextPane, window, cx| {
                    this.step_focus(true, window, cx)
                }),
            )
            .on_action(cx.listener(|this, _: &FocusPreviousPane, window, cx| {
                this.step_focus(false, window, cx)
            }))
            .on_action(cx.listener(|this, _: &FocusNextPaneAtPrompt, window, cx| {
                this.step_focus_at_prompt(window, cx)
            }))
            .on_action(cx.listener(|this, _: &RenamePane, window, cx| {
                let pane = this.state.focused;
                this.rename_pane(pane, window, cx)
            }))
            .on_action(cx.listener(|this, _: &RenameTab, window, cx| this.rename_tab(window, cx)))
            .on_action(cx.listener(|this, _: &ToggleSendRightClicks, _, cx| {
                let pane = this.state.focused;
                this.toggle_right_clicks(pane, cx)
            }))
            .on_action(cx.listener(|this, _: &ClosePane, window, cx| {
                let pane = this.state.focused;
                this.close_pane(pane, window, cx)
            }))
            .child(
                canvas(
                    move |area, _, _| bounds.set(Some(area)),
                    move |_, _, window, _| {
                        // Window-wide, so a divider drag that leaves the tab
                        // keeps going and its release is not missed.
                        window.on_mouse_event({
                            let this = this.clone();
                            move |e: &MouseMoveEvent, phase, _, cx| {
                                if phase.bubble() && e.pressed_button == Some(MouseButton::Left) {
                                    let _ =
                                        this.update(cx, |this, cx| this.drag_to(e.position, cx));
                                }
                            }
                        });
                        window.on_mouse_event(move |e: &MouseUpEvent, phase, _, cx| {
                            if phase.bubble() && e.button == MouseButton::Left {
                                let _ = this.update(cx, |this, _| this.drag = None);
                            }
                        });
                    },
                )
                .absolute()
                .top_0()
                .left_0()
                .size_full(),
            );
        // One pane, or the first frame (no size yet): the focused pane fills
        // the tab, with no header.
        let area = self.area().filter(|_| self.state.is_split());
        let Some(area) = area else {
            let pane = self.state.focused;
            let view = self.focused_view().clone();
            return match self.render_banner(pane, &view, cx) {
                Some(banner) => root.child(
                    v_flex()
                        .size_full()
                        .child(banner)
                        .child(div().flex_1().min_h_0().child(view)),
                ),
                None => root.child(view),
            };
        };

        let pane_min = min_size(cell_size(window, cx));
        let here = self.keyboard_here(window, cx);
        let mut panes = Vec::new();
        for (pane, rect) in self.state.visible(area) {
            if let Some(view) = self.views.get(&pane) {
                let look = PaneLook::of(pane, self.state.focused, here);
                panes.push(
                    self.render_pane(pane, rect, view, look, window, cx)
                        .into_any_element(),
                );
            }
        }
        let dividers = match self.state.zoomed {
            Some(_) => Vec::new(),
            None => self.state.layout.dividers(area),
        };
        let strips = dividers.into_iter().enumerate().map(|(ix, divider)| {
            let half = DIVIDER_HIT / 2.;
            let span = divider.span;
            let (x, y, w, h) = match divider.dir {
                SplitDir::Right => (divider.at - half, span.y, DIVIDER_HIT, span.h),
                SplitDir::Down => (span.x, divider.at - half, span.w, DIVIDER_HIT),
            };
            let span_len = match divider.dir {
                SplitDir::Right => span.w,
                SplitDir::Down => span.h,
            };
            div()
                .id(("pane-divider", ix))
                .absolute()
                .left(px(x))
                .top(px(y))
                .w(px(w))
                .h(px(h))
                .occlude()
                .map(|el| match divider.dir {
                    SplitDir::Right => el.cursor_col_resize(),
                    SplitDir::Down => el.cursor_row_resize(),
                })
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, e: &MouseDownEvent, _, cx| {
                        if e.click_count >= 2 {
                            this.drag = None;
                            this.state.layout.even(&divider.path, span_len, pane_min);
                        } else {
                            this.drag = Some(Drag {
                                path: divider.path.clone(),
                                dir: divider.dir,
                                span,
                                pane_min,
                            });
                        }
                        cx.stop_propagation();
                        cx.notify();
                    }),
                )
        });
        root.children(panes).children(strips)
    }
}
