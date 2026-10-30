//! One terminal tab: its panes, each a `TerminalView`, arranged by a
//! `TabState` split layout. A tab with one pane shows it full size; with
//! more, each pane gets a header and a border, and the dividers between
//! them can be dragged. Right-clicking a pane opens its menu (rename,
//! split, zoom, right-click routing, close).

use std::cell::Cell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::base::POPUP_PRIORITY;
use gpui_kit::component::button::*;
use gpui_kit::component::input::{Escape, Input, InputEvent, InputState};
use gpui_kit::component::menu::{PopupMenu, PopupMenuItem};
use gpui_kit::component::{ActiveTheme as _, Icon, Sizable as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use sawit_core::terminal::{
    Direction, MIN_COLS, MIN_ROWS, PaneId, Rect, Removed, SplitDir, SplitPath, TabState,
    new_pane_id,
};

use super::view::{PAD_X, TerminalEvent, TerminalView, cell_size};
use super::{FocusDown, FocusLeft, FocusRight, FocusUp, SplitDown, SplitRight};

/// Height of a pane's header, its bottom border included (boxes are
/// border-box, so the border takes from this height).
pub const HEADER_H: Pixels = px(24.);
/// The line under a pane's header.
const HEADER_BORDER: Pixels = px(1.);
/// The border around each pane of a split tab.
const PANE_BORDER: Pixels = px(1.);
/// Slack for layout rounding, so a pane at its minimum never loses a
/// column or row to a fraction of a pixel.
const ROUNDING: f32 = 1.;
/// Width of the grab strip centred on each divider.
const DIVIDER_HIT: f32 = 5.;
/// Width of the field that renames a pane.
const RENAME_W: Pixels = px(160.);

/// The pane menu, in order: each label, with `Some(checked)` for a toggle.
pub fn pane_menu_items(zoomed: bool, send_right_clicks: bool) -> Vec<(&'static str, Option<bool>)> {
    vec![
        ("Rename pane", None),
        ("Split right", None),
        ("Split down", None),
        (if zoomed { "Unzoom" } else { "Zoom" }, None),
        ("Send right-clicks to pane", Some(send_right_clicks)),
        ("Close pane", None),
    ]
}

/// What a tab tells the panel.
pub enum TabEvent {
    /// A pane's title or state changed, so the tab strip needs a redraw.
    Changed,
    /// The tab's last pane closed.
    Closed,
}

/// A divider being dragged.
struct Drag {
    /// The split it belongs to.
    path: SplitPath,
    dir: SplitDir,
    /// The split's whole area, in the tab's coordinates.
    span: Rect,
    /// The smallest (width, height) any pane may get.
    pane_min: (f32, f32),
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
    /// The pane menu while it is open.
    menu: Option<Menu>,
    /// The pane being renamed, if any.
    renaming: Option<Rename>,
    /// The window the tab last drew in, where its pane menu opens.
    window: Option<AnyWindowHandle>,
}

/// An open pane menu.
struct Menu {
    view: Entity<PopupMenu>,
    /// Where it opens, in window coordinates.
    at: Point<Pixels>,
    _dismiss: Subscription,
}

/// A pane being renamed.
struct Rename {
    pane: PaneId,
    input: Entity<InputState>,
    _events: Subscription,
}

impl EventEmitter<TabEvent> for TerminalTab {}

impl TerminalTab {
    /// A tab holding `view` as its only pane.
    pub fn new(view: Entity<TerminalView>, group: Option<PathBuf>, cx: &mut Context<Self>) -> Self {
        let pane = new_pane_id();
        Self::watch(pane, &view, cx);
        Self {
            state: TabState::new(pane),
            views: HashMap::from([(pane, view)]),
            group,
            bounds: Rc::new(Cell::new(None)),
            drag: None,
            menu: None,
            renaming: None,
            window: None,
        }
    }

    /// Opens the pane menu when asked, and redraws the tab strip when the
    /// pane's title or state changes.
    fn watch(pane: PaneId, view: &Entity<TerminalView>, cx: &mut Context<Self>) {
        cx.subscribe(
            view,
            move |this, _, event: &TerminalEvent, cx| match event {
                TerminalEvent::MenuRequested(at) => this.request_menu(pane, *at, cx),
                TerminalEvent::TitleChanged | TerminalEvent::Exited => cx.emit(TabEvent::Changed),
            },
        )
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

    /// The name the user gave the focused pane, if any.
    pub fn pane_name(&self) -> Option<&str> {
        self.state
            .panes
            .get(&self.state.focused)
            .and_then(|meta| meta.name.as_deref())
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
        Self::watch(pane, &view, cx);
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

    /// Closes `pane`, ending its process. The last pane closes the tab: its
    /// terminal goes with the tab, which the panel drops on `Closed`.
    pub fn close_pane(&mut self, pane: PaneId, window: &mut Window, cx: &mut Context<Self>) {
        match self.state.close(pane) {
            Removed::NotFound => {}
            Removed::Last => {
                self.menu = None;
                self.renaming = None;
                cx.emit(TabEvent::Closed);
            }
            Removed::Focus(_) => {
                self.views.remove(&pane);
                if self.renaming.as_ref().is_some_and(|r| r.pane == pane) {
                    self.renaming = None;
                }
                self.focus(window, cx);
                cx.emit(TabEvent::Changed);
                cx.notify();
            }
        }
    }

    /// Opens the pane menu for `pane` once the window can be reached (an
    /// event from a pane comes without it).
    fn request_menu(&mut self, pane: PaneId, at: Point<Pixels>, cx: &mut Context<Self>) {
        let Some(handle) = self.window else {
            return;
        };
        let tab = cx.entity().downgrade();
        cx.defer(move |cx| {
            let _ = handle.update(cx, |_, window, cx| {
                tab.update(cx, |t, cx| t.open_menu(pane, at, window, cx))
            });
        });
    }

    /// Shows the pane menu for `pane` at `at` (window coordinates) and
    /// gives it the keyboard; dismissing it hands the keyboard back.
    fn open_menu(
        &mut self,
        pane: PaneId,
        at: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(view) = self.views.get(&pane) else {
            return;
        };
        let back = view.focus_handle(cx);
        let zoomed = self.state.zoomed == Some(pane);
        let send = self
            .state
            .panes
            .get(&pane)
            .is_some_and(|meta| meta.send_right_clicks);
        let tab = cx.entity().downgrade();
        let menu = PopupMenu::build(window, cx, move |mut menu, _, _| {
            menu = menu.action_context(back);
            for (label, checked) in pane_menu_items(zoomed, send) {
                let tab = tab.clone();
                let item = match checked {
                    Some(checked) => PopupMenuItem::new(label).checked(checked),
                    None => PopupMenuItem::new(label),
                };
                menu = menu.item(item.on_click(move |_, window, cx| {
                    let _ = tab.update(cx, |t, cx| t.run_menu_item(pane, label, window, cx));
                }));
            }
            menu
        });
        menu.focus_handle(cx).focus(window, cx);
        let dismiss = cx.subscribe(&menu, |this, menu, _: &DismissEvent, cx| {
            if this.menu.as_ref().is_some_and(|m| m.view == menu) {
                this.menu = None;
                cx.notify();
            }
        });
        self.menu = Some(Menu {
            view: menu,
            at,
            _dismiss: dismiss,
        });
        cx.notify();
    }

    /// Does what the pane menu item `label` says, to `pane`.
    fn run_menu_item(
        &mut self,
        pane: PaneId,
        label: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match label {
            "Rename pane" => self.start_rename(pane, window, cx),
            "Split right" => {
                self.focus_pane(pane, window, cx);
                window.dispatch_action(Box::new(SplitRight), cx);
            }
            "Split down" => {
                self.focus_pane(pane, window, cx);
                window.dispatch_action(Box::new(SplitDown), cx);
            }
            "Zoom" | "Unzoom" => self.toggle_zoom(pane, window, cx),
            "Send right-clicks to pane" => {
                self.state.toggle_right_clicks(pane);
                let send = self
                    .state
                    .panes
                    .get(&pane)
                    .is_some_and(|meta| meta.send_right_clicks);
                if let Some(view) = self.views.get(&pane) {
                    view.update(cx, |v, _| v.send_right_clicks = send);
                }
            }
            "Close pane" => self.close_pane(pane, window, cx),
            _ => {}
        }
    }

    /// Puts a field for `pane`'s name in its header (in the tab strip when
    /// the tab has one pane), filled with its title and selected. Enter
    /// names the pane; Escape or leaving the field keeps the old name.
    pub(super) fn start_rename(
        &mut self,
        pane: PaneId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(view) = self.views.get(&pane) else {
            return;
        };
        let title = self.pane_title(pane, view.read(cx));
        let input = cx.new(|cx| InputState::new(window, cx).default_value(title));
        input.update(cx, |s, cx| {
            s.focus(window, cx);
            s.select_all(window, cx);
        });
        let events = cx.subscribe_in(&input, window, move |this, input, e, window, cx| match e {
            InputEvent::PressEnter { .. } => {
                let name = input.read(cx).value();
                this.state.rename(pane, &name);
                this.end_rename(window, cx);
            }
            InputEvent::Blur => {
                this.renaming = None;
                cx.emit(TabEvent::Changed);
                cx.notify();
            }
            InputEvent::Change | InputEvent::Focus => {}
        });
        self.renaming = Some(Rename {
            pane,
            input,
            _events: events,
        });
        cx.emit(TabEvent::Changed);
        cx.notify();
    }

    /// Closes the rename field and gives the keyboard back to the pane.
    fn end_rename(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.renaming = None;
        self.focus(window, cx);
        cx.emit(TabEvent::Changed);
        cx.notify();
    }

    /// The field renaming `pane`, while it is open.
    pub fn rename_field(&self, tab: WeakEntity<Self>, pane: PaneId) -> Option<AnyElement> {
        let rename = self.renaming.as_ref().filter(|r| r.pane == pane)?;
        Some(
            div()
                .w(RENAME_W)
                .flex_none()
                .on_action(move |_: &Escape, window, cx| {
                    let _ = tab.update(cx, |t, cx| t.end_rename(window, cx));
                })
                // A click in the field must not reach the pane or tab under
                // it, which would take the keyboard and close the field.
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .child(Input::new(&rename.input).xsmall())
                .into_any_element(),
        )
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
            .set_ratio(&drag.path, ratio, len, drag.pane_min);
        cx.notify();
    }

    /// A pane's header: what runs there and where, and its buttons.
    fn render_header(
        &self,
        pane: PaneId,
        view: &Entity<TerminalView>,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let folder = view.update(cx, |v, cx| v.folder(cx)).map(|dir| tilde(&dir));
        let v = view.read(cx);
        let icon = if v.remote.is_some() {
            Lucide::Server
        } else if v.job.is_some() {
            Lucide::Play
        } else {
            Lucide::Terminal
        };
        let title = self.pane_title(pane, v);
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
            .border_b(HEADER_BORDER)
            .border_color(cx.theme().border)
            .child(Icon::new(icon).xsmall())
            .child(
                self.rename_field(cx.entity().downgrade(), pane)
                    .unwrap_or_else(|| div().flex_none().child(title).into_any_element()),
            )
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
                Button::new(("pane-close", pane))
                    .ghost()
                    .xsmall()
                    .icon(Lucide::X)
                    .tooltip("Close pane")
                    .on_click(
                        cx.listener(move |this, _, window, cx| this.close_pane(pane, window, cx)),
                    ),
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
            .border(PANE_BORDER)
            .border_color(border)
            .on_any_mouse_down(
                cx.listener(move |this, _, window, cx| this.focus_pane(pane, window, cx)),
            )
            .child(self.render_header(pane, view, cx))
            .child(div().flex_1().min_h_0().child(view.clone()))
    }
}

/// The smallest pane rect, in pixels, whose grid still gets 20 columns by
/// 4 rows: the grid plus the pane's border on both sides, the view's padding
/// left and right, and the header (which holds its own border) on top.
fn min_size(cell: Size<Pixels>) -> (f32, f32) {
    let border = 2. * f32::from(PANE_BORDER);
    (
        f32::from(cell.width) * f32::from(MIN_COLS) + 2. * f32::from(PAD_X) + border + ROUNDING,
        f32::from(cell.height) * f32::from(MIN_ROWS) + f32::from(HEADER_H) + border + ROUNDING,
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

        let pane_min = min_size(cell_size(window, cx));
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

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::time::{Duration, Instant};

    use gpui_kit::{
        AnyWindowHandle, App, AppContext as _, Bounds, Entity, Modifiers, MouseButton, Pixels,
        Point, TestAppContext, VisualTestContext, point, px,
    };
    use sawit_core::terminal::{Layout, MIN_COLS, MIN_ROWS, SplitDir};

    use super::super::TerminalPanel;

    use super::super::testing::{focus_terminal, init, open_panel};
    use super::pane_menu_items;

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

    /// Every pane's grid size in `tab`, after the last paint.
    fn grids(cx: &mut TestAppContext, panel: &Entity<TerminalPanel>) -> Vec<(u16, u16)> {
        cx.update(|cx| {
            let tab = panel.read(cx).tab(0).read(cx);
            tab.views()
                .map(|(_, v)| {
                    let g = v.read(cx).grid();
                    (g.cols, g.rows)
                })
                .collect()
        })
    }

    fn assert_big_enough(grids: &[(u16, u16)]) {
        for &(cols, rows) in grids {
            assert!(
                cols >= MIN_COLS && rows >= MIN_ROWS,
                "a pane is {cols}×{rows}, under {MIN_COLS}×{MIN_ROWS}"
            );
        }
    }

    /// Splits with `key` until the tab refuses; returns the pane count.
    fn split_until_refused(
        cx: &mut TestAppContext,
        w: AnyWindowHandle,
        panel: &Entity<TerminalPanel>,
        key: &str,
    ) -> usize {
        let count = |cx: &mut TestAppContext| {
            cx.update(|cx| panel.read(cx).tab(0).read(cx).views().count())
        };
        let mut last = count(cx);
        loop {
            cx.simulate_keystrokes(w, key);
            cx.run_until_parked();
            let now = count(cx);
            if now == last {
                return now;
            }
            assert!(now < 64, "splitting never stopped");
            last = now;
        }
    }

    #[gpui_kit::test]
    fn splitting_stops_before_a_pane_gets_too_small(cx: &mut TestAppContext) {
        cx.update(init);
        let (w, panel) = open_panel(cx);
        focus_terminal(cx, w, &panel);
        let count = split_until_refused(cx, w, &panel, "cmd-shift-d");
        assert!(count > 1, "the tab was never split");
        cx.update(|cx| {
            let tab = panel.read(cx).tab(0).read(cx);
            assert_eq!(tab.views.len(), count, "a refused pane's terminal was kept");
        });
        assert_big_enough(&grids(cx, &panel));

        let (w, panel) = open_panel(cx);
        focus_terminal(cx, w, &panel);
        assert!(split_until_refused(cx, w, &panel, "cmd-d") > 1);
        assert_big_enough(&grids(cx, &panel));
    }

    /// Drags the tab's first divider to `to` (window coordinates) and lets
    /// the panes repaint.
    fn drag_root_divider(
        cx: &mut TestAppContext,
        w: AnyWindowHandle,
        panel: &Entity<TerminalPanel>,
        to: impl Fn(Bounds<Pixels>) -> Point<Pixels>,
    ) {
        let tab = cx.update(|cx| panel.read(cx).tab(0).clone());
        cx.update_window(w, |_, window, cx| {
            let pane_min = super::min_size(super::cell_size(window, cx));
            tab.update(cx, |t, cx| {
                let divider = t.state.layout.dividers(t.area().unwrap()).remove(0);
                t.drag = Some(super::Drag {
                    path: divider.path,
                    dir: divider.dir,
                    span: divider.span,
                    pane_min,
                });
                let at = to(t.bounds.get().unwrap());
                t.drag_to(at, cx);
                t.drag = None;
            })
        })
        .unwrap();
        cx.run_until_parked();
    }

    #[gpui_kit::test]
    fn dragging_a_divider_keeps_20_columns_and_4_rows(cx: &mut TestAppContext) {
        cx.update(init);
        let (w, panel) = open_panel(cx);
        focus_terminal(cx, w, &panel);
        cx.simulate_keystrokes(w, "cmd-d");
        cx.run_until_parked();
        drag_root_divider(cx, w, &panel, |b| b.origin);
        assert_big_enough(&grids(cx, &panel));
        drag_root_divider(cx, w, &panel, |b| b.origin + point(b.size.width, px(0.)));
        assert_big_enough(&grids(cx, &panel));

        let (w, panel) = open_panel(cx);
        focus_terminal(cx, w, &panel);
        cx.simulate_keystrokes(w, "cmd-shift-d");
        cx.run_until_parked();
        drag_root_divider(cx, w, &panel, |b| b.origin);
        assert_big_enough(&grids(cx, &panel));
        drag_root_divider(cx, w, &panel, |b| b.origin + point(px(0.), b.size.height));
        assert_big_enough(&grids(cx, &panel));
    }

    #[gpui_kit::test]
    fn a_pane_header_follows_cd_without_more_output(cx: &mut TestAppContext) {
        cx.update(init);
        let (w, panel) = open_panel(cx);
        focus_terminal(cx, w, &panel);
        cx.simulate_keystrokes(w, "cmd-d");
        cx.run_until_parked();
        let view = cx.update(|cx| panel.read(cx).tab(0).read(cx).focused_view().clone());
        wait_until(cx, "the header shows a folder", |cx| {
            view.read(cx).cached_folder().is_some()
        });
        let dir = scratch_dir("header-cd");
        cx.update(|cx| view.read(cx).cd(&dir));
        // No more input: only the passing of time may bring the header up
        // to date.
        let deadline = Instant::now() + Duration::from_millis(1500);
        while cx.update(|cx| view.read(cx).cached_folder()).as_ref() != Some(&dir) {
            assert!(
                Instant::now() < deadline,
                "the header still shows the old folder"
            );
            std::thread::sleep(Duration::from_millis(50));
            cx.executor().advance_clock(Duration::from_millis(50));
            cx.run_until_parked();
        }
        let _ = std::fs::remove_dir_all(&dir);
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

    #[test]
    fn pane_menu_matches_herdr() {
        let labels: Vec<_> = pane_menu_items(false, true)
            .into_iter()
            .map(|(l, _)| l)
            .collect();
        assert_eq!(
            labels,
            [
                "Rename pane",
                "Split right",
                "Split down",
                "Zoom",
                "Send right-clicks to pane",
                "Close pane"
            ]
        );
        assert_eq!(pane_menu_items(true, false)[3].0, "Unzoom");
        assert_eq!(pane_menu_items(false, true)[4].1, Some(true));
    }

    #[gpui_kit::test]
    fn closing_panes_closes_the_tab_last(cx: &mut TestAppContext) {
        cx.update(init);
        let (w, panel) = open_panel(cx);
        focus_terminal(cx, w, &panel);
        cx.simulate_keystrokes(w, "cmd-d");
        cx.run_until_parked();
        let tab = cx.update(|cx| panel.read(cx).tab(0).clone());
        let panes = cx.update(|cx| tab.read(cx).state.layout.panes());
        assert_eq!(panes.len(), 2);

        let close = |cx: &mut TestAppContext, pane| {
            cx.update_window(w, |_, window, cx| {
                tab.update(cx, |t, cx| t.close_pane(pane, window, cx))
            })
            .unwrap();
            cx.run_until_parked();
        };
        let focused = cx.update(|cx| tab.read(cx).state.focused);
        close(cx, focused);
        cx.update(|cx| {
            let t = tab.read(cx);
            assert_eq!(t.views().count(), 1);
            assert_eq!(t.views.len(), 1, "the closed pane's terminal was kept");
            assert_eq!(t.state.focused, panes[0]);
        });

        close(cx, panes[0]);
        cx.update(|cx| assert_eq!(panel.read(cx).tab_count(), 0));
    }

    #[gpui_kit::test]
    fn renaming_a_pane_names_it_over_the_program_title(cx: &mut TestAppContext) {
        cx.update(init);
        let (w, panel) = open_panel(cx);
        focus_terminal(cx, w, &panel);
        let tab = cx.update(|cx| panel.read(cx).tab(0).clone());
        let pane = cx.update(|cx| tab.read(cx).state.focused);
        cx.update_window(w, |_, window, cx| {
            tab.update(cx, |t, cx| t.start_rename(pane, window, cx))
        })
        .unwrap();
        cx.run_until_parked();
        cx.simulate_input(w, "build");
        cx.simulate_keystrokes(w, "enter");
        cx.run_until_parked();
        cx.update(|cx| {
            let t = tab.read(cx);
            assert!(t.renaming.is_none());
            assert_eq!(t.title(cx), "build");
        });
        let back = cx
            .update_window(w, |_, window, cx| tab.read(cx).has_focus(window, cx))
            .unwrap();
        assert!(back, "the keyboard did not go back to the pane");
    }

    #[gpui_kit::test]
    fn right_clicks_open_the_pane_menu_unless_sent_on(cx: &mut TestAppContext) {
        cx.update(init);
        let (w, panel) = open_panel(cx);
        focus_terminal(cx, w, &panel);
        let tab = cx.update(|cx| panel.read(cx).tab(0).clone());
        let pane = cx.update(|cx| tab.read(cx).state.focused);
        let at = cx.update(|cx| tab.read(cx).bounds.get().unwrap().center());
        let mut vcx = VisualTestContext::from_window(w, cx);
        let right_click = |vcx: &mut VisualTestContext, modifiers: Modifiers| {
            vcx.simulate_mouse_down(at, MouseButton::Right, modifiers);
            vcx.simulate_mouse_up(at, MouseButton::Right, modifiers);
            vcx.run_until_parked();
        };
        let menu_open =
            |vcx: &mut VisualTestContext| vcx.update(|_, cx| tab.read(cx).menu.is_some());

        right_click(&mut vcx, Modifiers::default());
        assert!(menu_open(&mut vcx), "a right-click did not open the menu");

        vcx.update(|window, cx| {
            tab.update(cx, |t, cx| {
                t.run_menu_item(pane, "Send right-clicks to pane", window, cx);
                t.menu = None;
            })
        });
        let sent = vcx.update(|_, cx| tab.read(cx).focused_view().read(cx).send_right_clicks);
        assert!(sent, "the pane does not send right-clicks on");
        // The shell does not take the mouse, so the click goes nowhere.
        right_click(&mut vcx, Modifiers::default());
        assert!(
            !menu_open(&mut vcx),
            "the menu opened for a sent right-click"
        );

        right_click(&mut vcx, Modifiers::shift());
        assert!(
            menu_open(&mut vcx),
            "Shift+right-click did not open the menu"
        );
    }
}
