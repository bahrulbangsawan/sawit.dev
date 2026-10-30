//! One terminal tab: its panes, each a `TerminalView`, arranged by a
//! `TabState` split layout. A tab with one pane shows it full size; with
//! more, each pane gets a header and a border, and the dividers between
//! them can be dragged.

use std::cell::Cell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::*;
use gpui_kit::component::{ActiveTheme as _, Disableable as _, Icon, Sizable as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use sawit_core::terminal::{
    Direction, MIN_COLS, MIN_ROWS, PaneId, Rect, SplitDir, SplitPath, TabState, new_pane_id,
};

use super::view::{TerminalEvent, TerminalView, cell_size};
use super::{FocusDown, FocusLeft, FocusRight, FocusUp, SplitRight};

/// Height of a pane's header.
pub const HEADER_H: Pixels = px(24.);
/// Width of the grab strip centred on each divider.
const DIVIDER_HIT: f32 = 5.;

/// What a tab tells the panel.
pub enum TabEvent {
    /// A pane's title or state changed, so the tab strip needs a redraw.
    Changed,
    /// The tab's last pane closed.
    #[expect(dead_code, reason = "emitted once single panes can be closed")]
    Closed,
}

/// A divider being dragged.
struct Drag {
    /// The split it belongs to.
    path: SplitPath,
    dir: SplitDir,
    /// The split's whole area, in the tab's coordinates.
    span: Rect,
    /// The smallest each side may get, along `dir`.
    min: f32,
}

/// One tab of the terminal panel and the panes in it.
pub struct TerminalTab {
    /// Layout, focus, zoom and per-pane settings.
    pub state: TabState,
    /// Every pane's terminal; always holds exactly the layout's panes.
    views: HashMap<PaneId, Entity<TerminalView>>,
    /// The project whose group the tab is in (none for SSH or no project).
    pub group: Option<PathBuf>,
    /// The tab's area from the last paint, for laying out panes.
    bounds: Rc<Cell<Option<Bounds<Pixels>>>>,
    /// The divider under the mouse while it is dragged.
    drag: Option<Drag>,
}

impl EventEmitter<TabEvent> for TerminalTab {}

impl TerminalTab {
    /// A tab holding `view` as its only pane.
    pub fn new(view: Entity<TerminalView>, group: Option<PathBuf>, cx: &mut Context<Self>) -> Self {
        let pane = new_pane_id();
        Self::watch(&view, cx);
        Self {
            state: TabState::new(pane),
            views: HashMap::from([(pane, view)]),
            group,
            bounds: Rc::new(Cell::new(None)),
            drag: None,
        }
    }

    /// Redraws the tab strip when the pane's title or state changes.
    fn watch(view: &Entity<TerminalView>, cx: &mut Context<Self>) {
        cx.subscribe(view, |_, _, _: &TerminalEvent, cx| {
            cx.emit(TabEvent::Changed)
        })
        .detach();
    }

    /// The terminal of the pane that has focus.
    pub fn focused_view(&self) -> &Entity<TerminalView> {
        // The focused pane is always one of the layout's panes.
        &self.views[&self.state.focused]
    }

    /// Every pane and its terminal, in layout order.
    pub fn views(&self) -> impl Iterator<Item = (PaneId, &Entity<TerminalView>)> {
        self.state
            .layout
            .panes()
            .into_iter()
            .filter_map(|pane| self.views.get(&pane).map(|view| (pane, view)))
    }

    /// A pane's name, else its program's title.
    fn pane_title(&self, pane: PaneId, view: &TerminalView) -> SharedString {
        match self
            .state
            .panes
            .get(&pane)
            .and_then(|meta| meta.name.clone())
        {
            Some(name) => name.into(),
            None => view.title.clone(),
        }
    }

    /// The focused pane's name, else its program's title.
    pub fn title(&self, cx: &App) -> SharedString {
        self.pane_title(self.state.focused, self.focused_view().read(cx))
    }

    /// Gives the keyboard to the focused pane.
    pub fn focus(&self, window: &mut Window, cx: &mut App) {
        let handle = self.focused_view().focus_handle(cx);
        window.focus(&handle, cx);
    }

    /// One of the tab's panes has the keyboard.
    pub fn has_focus(&self, window: &Window, cx: &App) -> bool {
        self.views()
            .any(|(_, view)| view.focus_handle(cx).contains_focused(window, cx))
    }

    /// The tab's area from the last paint, in its own coordinates.
    fn area(&self) -> Option<Rect> {
        self.bounds.get().map(|b| Rect {
            x: 0.,
            y: 0.,
            w: f32::from(b.size.width),
            h: f32::from(b.size.height),
        })
    }

    /// Splits the focused pane and puts `view` in the new half, focused.
    /// Refused (false) before the first paint or when either half would be
    /// under 20 columns or 4 rows; `view` is then dropped, ending its process.
    pub fn add_pane(
        &mut self,
        dir: SplitDir,
        view: Entity<TerminalView>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(area) = self.area() else {
            return false;
        };
        let pane = new_pane_id();
        if !self
            .state
            .split(dir, pane, area, min_size(cell_size(window, cx)))
        {
            return false;
        }
        Self::watch(&view, cx);
        self.views.insert(pane, view);
        self.focus(window, cx);
        cx.emit(TabEvent::Changed);
        cx.notify();
        true
    }

    /// Focuses `pane` and gives it the keyboard.
    fn focus_pane(&mut self, pane: PaneId, window: &mut Window, cx: &mut Context<Self>) {
        if !self.views.contains_key(&pane) {
            return;
        }
        let changed = self.state.focused != pane;
        self.state.focus(pane);
        self.focus(window, cx);
        if changed {
            cx.emit(TabEvent::Changed);
        }
        cx.notify();
    }

    /// Focuses the neighbouring pane. Without one the key goes on to the
    /// next handler.
    fn move_focus(&mut self, dir: Direction, window: &mut Window, cx: &mut Context<Self>) {
        let next = self
            .area()
            .and_then(|area| self.state.layout.neighbor(self.state.focused, dir, area));
        match next {
            Some(pane) => self.focus_pane(pane, window, cx),
            None => cx.propagate(),
        }
    }

    /// Zooms `pane`, or unzooms it.
    fn toggle_zoom(&mut self, pane: PaneId, window: &mut Window, cx: &mut Context<Self>) {
        self.focus_pane(pane, window, cx);
        self.state.toggle_zoom();
        cx.notify();
    }

    /// Moves the dragged divider to `position` (window coordinates).
    fn drag_to(&mut self, position: Point<Pixels>, cx: &mut Context<Self>) {
        let (Some(drag), Some(bounds)) = (&self.drag, self.bounds.get()) else {
            return;
        };
        let (at, start, len) = match drag.dir {
            SplitDir::Right => (
                f32::from(position.x - bounds.origin.x),
                drag.span.x,
                drag.span.w,
            ),
            SplitDir::Down => (
                f32::from(position.y - bounds.origin.y),
                drag.span.y,
                drag.span.h,
            ),
        };
        let ratio = (at - start) / len;
        self.state
            .layout
            .set_ratio(&drag.path, ratio, len, drag.min);
        cx.notify();
    }

    /// A pane's header: what runs there and where, and its buttons.
    fn render_header(
        &self,
        pane: PaneId,
        view: &Entity<TerminalView>,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let v = view.read(cx);
        let icon = if v.remote.is_some() {
            Lucide::Server
        } else if v.job.is_some() {
            Lucide::Play
        } else {
            Lucide::Terminal
        };
        let title = self.pane_title(pane, v);
        let folder = v.folder().map(|dir| tilde(&dir));
        let zoomed = self.state.zoomed == Some(pane);
        let (zoom_icon, zoom_tip) = if zoomed {
            (Lucide::Minimize, "Unzoom")
        } else {
            (Lucide::Maximize, "Zoom")
        };
        h_flex()
            .h(HEADER_H)
            .flex_none()
            .pl_2()
            .pr_1()
            .gap_1()
            .text_xs()
            .bg(cx.theme().tab_bar)
            .border_b_1()
            .border_color(cx.theme().border)
            .child(Icon::new(icon).xsmall())
            .child(div().flex_none().child(title))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_ellipsis()
                    .text_color(cx.theme().muted_foreground)
                    .when_some(folder, |el, folder| el.child(folder)),
            )
            .child(
                Button::new(("pane-split", pane))
                    .ghost()
                    .xsmall()
                    .icon(Lucide::Columns2)
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
                    .tooltip(zoom_tip)
                    .on_click(
                        cx.listener(move |this, _, window, cx| this.toggle_zoom(pane, window, cx)),
                    ),
            )
            .child(
                // Closing a single pane comes with the pane menu.
                Button::new(("pane-close", pane))
                    .ghost()
                    .xsmall()
                    .icon(Lucide::X)
                    .tooltip("Close pane")
                    .disabled(true),
            )
    }

    /// One pane at `rect`: header, border and terminal.
    fn render_pane(
        &self,
        pane: PaneId,
        rect: Rect,
        view: &Entity<TerminalView>,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let border = if pane == self.state.focused {
            cx.theme().ring
        } else {
            cx.theme().border
        };
        v_flex()
            .id(("pane", pane))
            .absolute()
            .left(px(rect.x))
            .top(px(rect.y))
            .w(px(rect.w))
            .h(px(rect.h))
            .overflow_hidden()
            .border_1()
            .border_color(border)
            .on_any_mouse_down(
                cx.listener(move |this, _, window, cx| this.focus_pane(pane, window, cx)),
            )
            .child(self.render_header(pane, view, cx))
            .child(div().flex_1().min_h_0().child(view.clone()))
    }
}

/// The smallest pane, in pixels: 20 columns by 4 rows, plus its header.
fn min_size(cell: Size<Pixels>) -> (f32, f32) {
    (
        f32::from(cell.width) * f32::from(MIN_COLS),
        f32::from(cell.height) * f32::from(MIN_ROWS) + f32::from(HEADER_H),
    )
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

impl Render for TerminalTab {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let bounds = self.bounds.clone();
        let this = cx.entity().downgrade();
        let root = div()
            .relative()
            .size_full()
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
            return root.child(self.focused_view().clone());
        };

        let (min_w, min_h) = min_size(cell_size(window, cx));
        let mut panes = Vec::new();
        for (pane, rect) in self.state.visible(area) {
            if let Some(view) = self.views.get(&pane) {
                panes.push(self.render_pane(pane, rect, view, cx).into_any_element());
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
            let min = match divider.dir {
                SplitDir::Right => min_w,
                SplitDir::Down => min_h,
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
                            this.state.layout.even(&divider.path);
                        } else {
                            this.drag = Some(Drag {
                                path: divider.path.clone(),
                                dir: divider.dir,
                                span,
                                min,
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

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::time::{Duration, Instant};

    use gpui_kit::{App, AppContext as _, TestAppContext};
    use sawit_core::terminal::{Layout, SplitDir};

    use super::super::testing::{focus_terminal, init, open_panel};

    /// A fresh, empty folder under the temp dir, by its real path (what a
    /// process's cwd reports).
    fn scratch_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("sawit-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir.canonicalize().unwrap()
    }

    /// Polls `ok` (the shells run on real threads) for up to five seconds.
    fn wait_until(cx: &mut TestAppContext, what: &str, ok: impl Fn(&App) -> bool) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while !cx.update(|cx| ok(cx)) {
            assert!(Instant::now() < deadline, "timed out waiting until {what}");
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    #[gpui_kit::test]
    fn a_new_panel_has_one_tab_of_one_pane(cx: &mut TestAppContext) {
        cx.update(init);
        let (_w, panel) = open_panel(cx);
        cx.update(|cx| {
            let p = panel.read(cx);
            assert_eq!(p.tab_count(), 1);
            let tab = p.tab(0).read(cx);
            assert_eq!(tab.views().count(), 1);
            assert!(!tab.state.is_split());
        });
    }

    #[gpui_kit::test]
    fn cmd_d_splits_right_and_focuses_the_new_pane(cx: &mut TestAppContext) {
        cx.update(init);
        let (w, panel) = open_panel(cx);
        focus_terminal(cx, w, &panel);
        cx.simulate_keystrokes(w, "cmd-d");
        cx.run_until_parked();
        cx.update(|cx| {
            let tab = panel.read(cx).tab(0).read(cx);
            assert_eq!(tab.views().count(), 2);
            assert!(matches!(
                tab.state.layout,
                Layout::Split {
                    dir: SplitDir::Right,
                    ..
                }
            ));
            assert_eq!(tab.state.focused, tab.state.layout.panes()[1]);
        });
    }

    #[gpui_kit::test]
    fn cmd_shift_d_splits_down_and_cmd_alt_arrows_move_focus(cx: &mut TestAppContext) {
        cx.update(init);
        let (w, panel) = open_panel(cx);
        focus_terminal(cx, w, &panel);
        cx.simulate_keystrokes(w, "cmd-shift-d");
        cx.run_until_parked();
        let panes = cx.update(|cx| panel.read(cx).tab(0).read(cx).state.layout.panes());
        assert_eq!(panes.len(), 2);
        cx.simulate_keystrokes(w, "cmd-alt-up");
        cx.update(|cx| assert_eq!(panel.read(cx).tab(0).read(cx).state.focused, panes[0]));
        cx.simulate_keystrokes(w, "cmd-alt-down");
        cx.update(|cx| assert_eq!(panel.read(cx).tab(0).read(cx).state.focused, panes[1]));
    }

    #[gpui_kit::test]
    fn splitting_stops_before_a_pane_gets_too_short(cx: &mut TestAppContext) {
        cx.update(init);
        let (w, panel) = open_panel(cx);
        focus_terminal(cx, w, &panel);
        cx.simulate_keystrokes(
            w,
            "cmd-shift-d cmd-shift-d cmd-shift-d cmd-shift-d cmd-shift-d",
        );
        cx.run_until_parked();
        let (min_h, count) = cx
            .update_window(w, |_, window, cx| {
                let (_, min_h) = super::min_size(super::cell_size(window, cx));
                (min_h, panel.read(cx).tab(0).read(cx).views().count())
            })
            .unwrap();
        cx.simulate_keystrokes(w, "cmd-shift-d");
        cx.run_until_parked();
        cx.update(|cx| {
            let tab = panel.read(cx).tab(0).read(cx);
            assert!(count > 1, "the tab was never split");
            assert_eq!(tab.views().count(), count, "a too-short split went through");
            assert_eq!(tab.views.len(), count, "a refused pane's terminal was kept");
            let area = tab.area().unwrap();
            for (_, rect) in tab.state.layout.rects(area) {
                assert!(rect.h >= min_h, "{} is under {min_h}", rect.h);
            }
        });
    }

    #[gpui_kit::test]
    fn a_split_starts_in_the_focused_panes_folder(cx: &mut TestAppContext) {
        cx.update(init);
        let (w, panel) = open_panel(cx);
        let dir = scratch_dir("split-cwd");
        cx.update_window(w, |_, window, cx| {
            panel.update(cx, |p, cx| p.open_shell_in(dir.clone(), window, cx))
        })
        .unwrap();
        cx.run_until_parked();
        let tab = cx.update(|cx| {
            let p = panel.read(cx);
            p.tab(p.active_index()).clone()
        });
        wait_until(cx, "the shell runs in the folder", |cx| {
            tab.read(cx).focused_view().read(cx).cwd().as_ref() == Some(&dir)
        });

        focus_terminal(cx, w, &panel);
        cx.simulate_keystrokes(w, "cmd-d");
        cx.run_until_parked();
        let new = cx.update(|cx| {
            let t = tab.read(cx);
            assert_eq!(t.views().count(), 2);
            t.focused_view().clone()
        });
        wait_until(cx, "the new pane runs in the folder", |cx| {
            new.read(cx).cwd().as_ref() == Some(&dir)
        });
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[gpui_kit::test]
    fn a_split_from_a_deleted_folder_starts_in_the_project(cx: &mut TestAppContext) {
        cx.update(init);
        let (w, panel) = open_panel(cx);
        let project = scratch_dir("split-project");
        let gone = project.join("gone");
        std::fs::create_dir(&gone).unwrap();
        cx.update(|cx| {
            panel.update(cx, |p, cx| {
                p.set_projects(vec![project.clone()], Some(project.clone()), cx)
            })
        });
        cx.update_window(w, |_, window, cx| {
            panel.update(cx, |p, cx| p.open_shell_in(gone.clone(), window, cx))
        })
        .unwrap();
        cx.run_until_parked();
        let tab = cx.update(|cx| {
            let p = panel.read(cx);
            p.tab(p.active_index()).clone()
        });
        cx.update(|cx| assert_eq!(tab.read(cx).group.as_ref(), Some(&project)));
        wait_until(cx, "the shell runs in the inner folder", |cx| {
            tab.read(cx).focused_view().read(cx).cwd().as_ref() == Some(&gone)
        });
        std::fs::remove_dir(&gone).unwrap();

        focus_terminal(cx, w, &panel);
        cx.simulate_keystrokes(w, "cmd-d");
        cx.run_until_parked();
        let new = cx.update(|cx| {
            let t = tab.read(cx);
            assert_eq!(t.views().count(), 2);
            t.focused_view().clone()
        });
        wait_until(cx, "the new pane runs in the project", |cx| {
            new.read(cx).cwd().as_ref() == Some(&project)
        });
        let _ = std::fs::remove_dir_all(&project);
    }
}
