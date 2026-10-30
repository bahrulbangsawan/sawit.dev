//! One terminal: draws `core::terminal` snapshots on a canvas (like Zed's
//! terminal element) and forwards keyboard, mouse and paste input. Dragging
//! selects text and copies it on release; Shift+drag selects even while a
//! program (herdr, vim) has the mouse. Dropped files are typed as paths.

use std::cell::Cell;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::{Duration, Instant};

use futures::StreamExt;
use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::{ActiveTheme as _, Icon, Sizable as _};
use gpui_kit::*;
use sawit_core::terminal::{
    CellView, CursorView, GridSize, KeyInput, MouseAction, MouseButton as TermButton, Palette, Rgb,
    ScrollAccumulator, SelectKind, Session, Snapshot, TermEvent, drop_bytes, key_to_bytes,
    mouse_report, paste_bytes,
};

use super::{FocusDown, FocusLeft, FocusRight, FocusUp, SplitDown, SplitRight, ZoomPane};
use crate::files::DraggedFile;

const FONT_SIZE: f32 = 13.;
const LINE_HEIGHT: f32 = 1.3;
/// How long the "Copied to clipboard" toast stays up.
const TOAST: Duration = Duration::from_millis(1400);
/// Space left and right of the grid.
pub(super) const PAD_X: Pixels = px(8.);
/// How long a pane header shows a folder before asking the OS again.
const FOLDER_TTL: Duration = Duration::from_millis(500);

/// Grid origin and cell size on screen.
type GridLayout = (Point<Pixels>, Size<Pixels>);

/// Key bindings run before `on_key_down`, so an outer binding for a key the
/// shell needs would swallow it: Root's Tab/Shift-Tab move focus out of the
/// terminal (and Tab is completion), Root's Cmd-C copies a stale selection
/// elsewhere in the window. Unbinding them here hands them to `on_key`.
/// App shortcuts (Ctrl-Tab, Cmd-…) stay with the workspace. The pane keys
/// (split, move focus, zoom) live here too, so they only act in a terminal.
pub(super) fn bindings() -> Vec<KeyBinding> {
    let ctx = Some("Terminal");
    vec![
        KeyBinding::new("tab", NoAction, ctx),
        KeyBinding::new("shift-tab", NoAction, ctx),
        KeyBinding::new("cmd-c", NoAction, ctx),
        KeyBinding::new("cmd-d", SplitRight, ctx),
        KeyBinding::new("cmd-shift-d", SplitDown, ctx),
        KeyBinding::new("cmd-alt-left", FocusLeft, ctx),
        KeyBinding::new("cmd-alt-right", FocusRight, ctx),
        KeyBinding::new("cmd-alt-up", FocusUp, ctx),
        KeyBinding::new("cmd-alt-down", FocusDown, ctx),
        KeyBinding::new("cmd-shift-enter", ZoomPane, ctx),
    ]
}

/// The size of one grid cell: the mono font's advance by its line height.
pub(super) fn cell_size(window: &Window, cx: &App) -> Size<Pixels> {
    let font = font(cx.theme().mono_font_family.clone());
    let font_id = window.text_system().resolve_font(&font);
    let font_size = px(FONT_SIZE);
    let cell_width = window
        .text_system()
        .advance(font_id, font_size, 'm')
        .map(|s| s.width)
        .unwrap_or(px(8.));
    size(cell_width, (font_size * LINE_HEIGHT).round())
}

pub enum TerminalEvent {
    TitleChanged,
    Exited,
    /// A right-click asked for the pane menu, at this window position.
    MenuRequested(Point<Pixels>),
}

/// Where a right-click goes.
pub enum RightClick {
    /// Opens the pane menu.
    Menu,
    /// Goes to the program as a mouse report.
    Program,
    /// Goes nowhere: the pane sends right-clicks on, but its program does
    /// not take the mouse.
    Nothing,
}

/// Where a right-click goes. Shift always opens the menu; otherwise it goes
/// to the program when the pane is set to send right-clicks on.
pub fn right_click_target(
    send_right_clicks: bool,
    shift: bool,
    program_has_mouse: bool,
) -> RightClick {
    if shift || !send_right_clicks {
        RightClick::Menu
    } else if program_has_mouse {
        RightClick::Program
    } else {
        RightClick::Nothing
    }
}

impl EventEmitter<TerminalEvent> for TerminalView {}

pub struct TerminalView {
    session: Rc<Session>,
    palette: Rc<Palette>,
    focus: FocusHandle,
    pub title: SharedString,
    pub exited: bool,
    /// Runs the user's shell (not herdr), so commands can be typed into it.
    pub shell: bool,
    /// Something was typed, pasted or run here; a fresh tab can be replaced.
    pub touched: bool,
    /// An SSH session: which server, and how to connect again.
    pub remote: Option<super::Remote>,
    /// Started from the Run panel: which script, under its name.
    pub job: Option<super::JobTag>,
    /// Right-clicks go to the program instead of opening the pane menu
    /// (the tab keeps this in step with the pane's settings).
    pub send_right_clicks: bool,
    /// Grid origin and cell size from the last paint, for mouse mapping.
    layout: Rc<Cell<Option<GridLayout>>>,
    pressed: Option<TermButton>,
    /// The left button is down and dragging extends the selection.
    selecting: bool,
    scroll: ScrollAccumulator,
    /// Shows the copy toast while set; finishing hides it.
    toast: Option<Task<()>>,
    /// The last `cwd()` answer for the pane header, and when it was asked.
    folder: Option<(Instant, Option<PathBuf>)>,
    /// Redraws once the cached folder is due to be asked again.
    folder_refresh: Option<Task<()>>,
    _events: Task<()>,
}

fn hsla(c: Rgb) -> Hsla {
    Rgba {
        r: c.r as f32 / 255.,
        g: c.g as f32 / 255.,
        b: c.b as f32 / 255.,
        a: 1.,
    }
    .into()
}

impl TerminalView {
    pub fn new(
        session: Session,
        events: futures::channel::mpsc::UnboundedReceiver<TermEvent>,
        title: impl Into<SharedString>,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut events = events;
        let task = cx.spawn(async move |this, cx| {
            while let Some(first) = events.next().await {
                let mut batch = vec![first];
                while let Ok(e) = events.try_recv() {
                    batch.push(e);
                }
                let alive = this.update(cx, |this, cx| {
                    for event in batch {
                        match event {
                            TermEvent::Wakeup | TermEvent::Bell => {}
                            TermEvent::Title(t) => {
                                if !t.is_empty() {
                                    this.title = t.into();
                                    cx.emit(TerminalEvent::TitleChanged);
                                }
                            }
                            TermEvent::Copy(text) => this.copy(text, cx),
                            TermEvent::Exit => {
                                this.exited = true;
                                cx.emit(TerminalEvent::Exited);
                            }
                        }
                    }
                    cx.notify();
                });
                if alive.is_err() {
                    return;
                }
            }
        });
        Self {
            session: Rc::new(session),
            palette: Rc::new(Palette::default()),
            focus: cx.focus_handle(),
            title: title.into(),
            exited: false,
            shell: false,
            touched: false,
            remote: None,
            job: None,
            send_right_clicks: false,
            layout: Rc::new(Cell::new(None)),
            pressed: None,
            selecting: false,
            scroll: ScrollAccumulator::default(),
            toast: None,
            folder: None,
            folder_refresh: None,
            _events: task,
        }
    }

    pub fn focus(&self, window: &mut Window, cx: &mut App) {
        window.focus(&self.focus, cx);
    }

    /// Sends raw input to the program, jumping back to the live screen.
    pub fn write(&self, bytes: Vec<u8>) {
        self.session.clear_selection();
        self.session.write(bytes);
        self.session.scroll_to_bottom();
    }

    /// The last `lines` lines of scrollback and screen as plain text.
    pub fn text_tail(&self, lines: usize) -> String {
        self.session.text_tail(lines)
    }

    /// Moves an idle shell into `dir` without showing the command.
    pub fn cd(&self, dir: &std::path::Path) {
        self.session.clear_selection();
        self.session.cd(dir);
        self.session.scroll_to_bottom();
    }

    /// The folder the program is in now. Asks the OS on every call.
    pub fn cwd(&self) -> Option<PathBuf> {
        self.session.cwd()
    }

    /// The program's folder for display. Asks the OS at most every 500 ms,
    /// since headers redraw with every frame of output. A cached answer
    /// schedules one redraw for when it expires, so after a `cd` the header
    /// settles on the new folder even if no more output comes.
    pub fn folder(&mut self, cx: &mut Context<Self>) -> Option<PathBuf> {
        if let Some((at, dir)) = &self.folder
            && at.elapsed() < FOLDER_TTL
        {
            if self.folder_refresh.is_none() {
                let wait = FOLDER_TTL.saturating_sub(at.elapsed());
                self.folder_refresh = Some(cx.spawn(async move |this, cx| {
                    cx.background_executor().timer(wait).await;
                    let _ = this.update(cx, |this, cx| {
                        this.folder_refresh = None;
                        cx.notify();
                    });
                }));
            }
            return dir.clone();
        }
        let dir = self.cwd();
        self.folder = Some((Instant::now(), dir.clone()));
        dir
    }

    /// The folder the header shows now, without asking the OS (for tests).
    #[cfg(test)]
    pub(super) fn cached_folder(&self) -> Option<PathBuf> {
        self.folder.as_ref().and_then(|(_, dir)| dir.clone())
    }

    /// The grid size the terminal was last laid out at (for tests).
    #[cfg(test)]
    pub(super) fn grid(&self) -> GridSize {
        self.session.size()
    }

    /// The shell is waiting at its prompt, not running a command.
    pub fn at_prompt(&self) -> bool {
        self.shell && !self.exited && self.session.at_prompt()
    }

    /// The command the shell is running, by process group.
    pub fn foreground_job(&self) -> Option<u32> {
        if self.exited {
            return None;
        }
        self.session.foreground_job()
    }

    fn modes(&self) -> sawit_core::terminal::Modes {
        self.session.modes()
    }

    fn on_key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let ks = &event.keystroke;
        let m = &ks.modifiers;
        if m.platform && ks.key == "v" {
            if let Some(text) = cx.read_from_clipboard().and_then(|c| c.text()) {
                self.touched = true;
                self.write(paste_bytes(&text, self.modes()));
                cx.notify();
            }
            cx.stop_propagation();
            return;
        }
        if m.platform && ks.key == "c" {
            if let Some(text) = self.session.selection_text() {
                self.copy(text, cx);
                cx.stop_propagation();
            }
            return;
        }
        let input = KeyInput {
            key: ks.key.clone(),
            text: ks.key_char.clone(),
            ctrl: m.control,
            alt: m.alt,
            shift: m.shift,
            cmd: m.platform,
        };
        if let Some(bytes) = key_to_bytes(&input, self.modes()) {
            self.touched = true;
            self.write(bytes);
            cx.notify();
            cx.stop_propagation();
            window.prevent_default();
        }
    }

    /// Grid cell under `position`, clamped to the screen, and whether the
    /// mouse is over its right half.
    fn cell_at(&self, position: Point<Pixels>) -> Option<(usize, usize, bool)> {
        let (origin, cell) = self.layout.get()?;
        let size = self.session.size();
        let x = ((position.x - origin.x) / cell.width).max(0.);
        let row = ((position.y - origin.y) / cell.height).floor().max(0.) as usize;
        let col = x.floor() as usize;
        let past_end = col >= size.cols as usize;
        Some((
            col.min(size.cols as usize - 1),
            row.min(size.rows as usize - 1),
            past_end || x.fract() >= 0.5,
        ))
    }

    fn start_selection(&mut self, position: Point<Pixels>, clicks: usize) {
        if let Some((col, row, right)) = self.cell_at(position) {
            self.session
                .select_start(col, row, right, SelectKind::from_clicks(clicks));
            self.selecting = true;
        }
    }

    fn extend_selection(&mut self, position: Point<Pixels>, cx: &mut Context<Self>) {
        if self.selecting
            && let Some((col, row, right)) = self.cell_at(position)
        {
            self.session.select_extend(col, row, right);
            cx.notify();
        }
    }

    /// Ends a drag: whatever got selected goes to the clipboard.
    fn finish_selection(&mut self, cx: &mut Context<Self>) {
        if !std::mem::take(&mut self.selecting) {
            return;
        }
        if let Some(text) = self.session.selection_text() {
            self.copy(text, cx);
        }
        cx.notify();
    }

    /// Puts `text` on the clipboard and briefly says so.
    fn copy(&mut self, text: String, cx: &mut Context<Self>) {
        cx.write_to_clipboard(ClipboardItem::new_string(text));
        self.toast = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(TOAST).await;
            let _ = this.update(cx, |this, cx| {
                this.toast = None;
                cx.notify();
            });
        }));
        cx.notify();
    }

    /// Types dropped files (from Finder or the file tree) as quoted paths.
    fn drop_paths(&mut self, paths: &[PathBuf], window: &mut Window, cx: &mut Context<Self>) {
        if paths.is_empty() || self.exited {
            return;
        }
        self.touched = true;
        self.write(drop_bytes(paths, self.modes()));
        self.focus(window, cx);
        cx.notify();
    }

    fn send_mouse(&self, action: MouseAction, position: Point<Pixels>, shift: bool) -> bool {
        let Some((col, row, _)) = self.cell_at(position) else {
            return false;
        };
        match mouse_report(action, col, row, shift, self.modes()) {
            Some(bytes) => {
                self.session.write(bytes);
                true
            }
            None => false,
        }
    }

    fn on_scroll(&mut self, event: &ScrollEvent, _: &mut Window, cx: &mut Context<Self>) {
        let Some((_, cell)) = self.layout.get() else {
            return;
        };
        let pixels = f32::from(event.delta.pixel_delta(cell.height).y);
        let lines = self.scroll.add(pixels, f32::from(cell.height));
        if lines == 0 {
            return;
        }
        let modes = self.modes();
        let action = if lines > 0 {
            MouseAction::WheelUp
        } else {
            MouseAction::WheelDown
        };
        if modes.mouse_report || modes.mouse_drag || modes.mouse_motion {
            for _ in 0..lines.unsigned_abs().min(10) {
                self.send_mouse(action, event.position, false);
            }
        } else if modes.alt_screen && modes.alternate_scroll {
            let arrow: &[u8] = match (lines > 0, modes.app_cursor) {
                (true, true) => b"\x1bOA",
                (true, false) => b"\x1b[A",
                (false, true) => b"\x1bOB",
                (false, false) => b"\x1b[B",
            };
            self.session
                .write(arrow.repeat(lines.unsigned_abs() as usize));
        } else {
            self.session.scroll(lines);
        }
        cx.notify();
    }
}

fn to_term_button(button: MouseButton) -> Option<TermButton> {
    match button {
        MouseButton::Left => Some(TermButton::Left),
        MouseButton::Middle => Some(TermButton::Middle),
        MouseButton::Right => Some(TermButton::Right),
        _ => None,
    }
}

/// Where and how to paint a snapshot.
struct Painter<'a> {
    palette: &'a Palette,
    origin: Point<Pixels>,
    cell: Size<Pixels>,
    font: &'a Font,
    focused: bool,
    selection: Hsla,
}

/// Paints one snapshot: backgrounds, text runs (glyphs forced onto the cell
/// grid), then the cursor.
fn paint_snapshot(snap: &Snapshot, p: &Painter, window: &mut Window, cx: &mut App) {
    let (palette, origin, cell, font, focused) = (p.palette, p.origin, p.cell, p.font, p.focused);
    let background = |c: &CellView| {
        if c.selected {
            Some(p.selection)
        } else {
            (c.bg != palette.background).then(|| hsla(c.bg))
        }
    };

    let font_size = px(FONT_SIZE);
    for (row, line) in snap.lines.iter().enumerate() {
        let y = origin.y + cell.height * row as f32;
        // Backgrounds, merged across equal neighbours.
        let mut col = 0;
        while col < line.len() {
            let bg = background(&line[col]);
            let start = col;
            while col < line.len() && background(&line[col]) == bg {
                col += 1;
            }
            if let Some(bg) = bg {
                let bounds = Bounds::new(
                    point(origin.x + cell.width * start as f32, y),
                    size(cell.width * (col - start) as f32, cell.height),
                );
                window.paint_quad(fill(bounds, bg));
            }
        }

        // Text: one shaped line per row, a run per style change. Trailing
        // blanks (most of a typical row) are not shaped at all.
        let Some(last) = line
            .iter()
            .rposition(|c| (!c.spacer && c.ch != ' ') || c.underline || c.strikeout)
        else {
            continue;
        };
        let line = &line[..=last];
        let mut text = String::with_capacity(line.len());
        let mut runs: Vec<TextRun> = Vec::new();
        // Runs are compared by this small key; building a Font (and its
        // Arc clones) per cell was most of the paint time.
        let mut run_key = None;
        for c in line {
            let ch = if c.spacer { ' ' } else { c.ch };
            let len = ch.len_utf8();
            text.push(ch);
            let key = (c.fg, c.bold, c.italic, c.underline, c.strikeout);
            if run_key == Some(key)
                && let Some(run) = runs.last_mut()
            {
                run.len += len;
                continue;
            }
            run_key = Some(key);
            let mut run_font = font.clone();
            if c.bold {
                run_font.weight = FontWeight::BOLD;
            }
            if c.italic {
                run_font.style = FontStyle::Italic;
            }
            let color = hsla(c.fg);
            runs.push(TextRun {
                len,
                font: run_font,
                color,
                background_color: None,
                underline: c.underline.then(|| UnderlineStyle {
                    thickness: px(1.),
                    color: Some(color),
                    wavy: false,
                }),
                strikethrough: c.strikeout.then(|| StrikethroughStyle {
                    thickness: px(1.),
                    color: Some(color),
                }),
            });
        }
        if text.trim().is_empty() {
            continue;
        }
        let shaped =
            window
                .text_system()
                .shape_line(text.into(), font_size, &runs, Some(cell.width));
        let _ = shaped.paint(
            point(origin.x, y),
            cell.height,
            TextAlign::Left,
            None,
            window,
            cx,
        );
    }

    if let Some((row, col)) = snap.cursor {
        let at = point(
            origin.x + cell.width * col as f32,
            origin.y + cell.height * row as f32,
        );
        let color = hsla(palette.cursor);
        let quad = match (snap.cursor_shape, focused) {
            (CursorView::Hidden, _) => None,
            (CursorView::Beam, _) => Some(fill(Bounds::new(at, size(px(2.), cell.height)), color)),
            (CursorView::Underline, _) => Some(fill(
                Bounds::new(
                    point(at.x, at.y + cell.height - px(2.)),
                    size(cell.width, px(2.)),
                ),
                color,
            )),
            (CursorView::Block, true) => Some(fill(Bounds::new(at, cell), color.opacity(0.6))),
            (CursorView::Block, false) => {
                Some(outline(Bounds::new(at, cell), color, BorderStyle::Solid))
            }
        };
        if let Some(quad) = quad {
            window.paint_quad(quad);
        }
    }
}

impl Render for TerminalView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let font = font(cx.theme().mono_font_family.clone());
        let cell = cell_size(window, cx);
        let session = self.session.clone();
        let palette = self.palette.clone();
        let layout = self.layout.clone();
        let focused = self.focus.is_focused(window);
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
            .relative()
            .key_context("Terminal")
            .track_focus(&self.focus)
            .size_full()
            .px(PAD_X)
            .bg(hsla(self.palette.background))
            .on_key_down(cx.listener(Self::on_key))
            .on_scroll(cx.listener(Self::on_scroll))
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

impl Focusable for TerminalView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

#[cfg(test)]
mod tests {
    use gpui_kit::{
        App, AppContext as _, Context, FocusHandle, IntoElement, KeyContext, KeyDownEvent,
        Keystroke, ParentElement as _, Render, Styled as _, TestAppContext, Window, div,
        prelude::*,
    };

    /// Keys a shell or TUI (Claude Code, vim, fzf, readline) needs.
    const SHELL_KEYS: &[&str] = &[
        "tab",
        "shift-tab",
        "enter",
        "shift-enter",
        "alt-enter",
        "escape",
        "backspace",
        "alt-backspace",
        "delete",
        "space",
        "ctrl-space",
        "up",
        "down",
        "left",
        "right",
        "shift-up",
        "alt-left",
        "alt-right",
        "ctrl-left",
        "ctrl-right",
        "home",
        "end",
        "pageup",
        "pagedown",
        "f1",
        "f5",
        "f12",
        "a",
        "shift-a",
        "/",
        "alt-f",
        "alt-b",
        "alt-d",
        "alt-.",
        "ctrl-a",
        "ctrl-b",
        "ctrl-c",
        "ctrl-d",
        "ctrl-e",
        "ctrl-f",
        "ctrl-g",
        "ctrl-h",
        "ctrl-j",
        "ctrl-k",
        "ctrl-l",
        "ctrl-n",
        "ctrl-o",
        "ctrl-p",
        "ctrl-r",
        "ctrl-s",
        "ctrl-t",
        "ctrl-u",
        "ctrl-v",
        "ctrl-w",
        "ctrl-x",
        "ctrl-y",
        "ctrl-z",
        "ctrl-\\",
        "ctrl-]",
        "ctrl-[",
        "ctrl-_",
        "cmd-c",
    ];

    /// Non-⌘ keys the app deliberately takes over a focused terminal.
    const APP_KEYS: &[&str] = &["ctrl-tab", "ctrl-shift-tab"];

    fn init(cx: &mut App) {
        gpui_kit::init(cx);
        crate::workspace::init(cx);
        super::super::init(cx);
    }

    fn stack(contexts: &[&str]) -> Vec<KeyContext> {
        contexts
            .iter()
            .map(|c| KeyContext::parse(c).unwrap())
            .collect()
    }

    fn name(k: &Keystroke) -> String {
        let m = &k.modifiers;
        let mut s = String::new();
        for (on, p) in [
            (m.control, "ctrl-"),
            (m.alt, "alt-"),
            (m.shift, "shift-"),
            (m.platform, "cmd-"),
        ] {
            if on {
                s.push_str(p);
            }
        }
        s + &k.key
    }

    #[gpui_kit::test]
    fn no_binding_takes_a_shell_key_from_the_terminal(cx: &mut TestAppContext) {
        cx.update(init);
        let terminal = stack(&["Root", "Workspace", "Terminal"]);
        cx.update(|cx| {
            let keymap = cx.key_bindings();
            let keymap = keymap.borrow();
            for key in SHELL_KEYS {
                let (bindings, pending) =
                    keymap.bindings_for_input(&[Keystroke::parse(key).unwrap()], &terminal);
                let names: Vec<_> = bindings.iter().map(|b| b.action().name()).collect();
                assert!(names.is_empty() && !pending, "{key} is taken by {names:?}");
            }
            // Whatever gets bound later: only ⌘ shortcuts and APP_KEYS may
            // beat the shell.
            for binding in keymap.bindings() {
                let keys: Vec<Keystroke> = binding
                    .keystrokes()
                    .iter()
                    .map(|k| Keystroke {
                        modifiers: *k.modifiers(),
                        key: k.key().to_string(),
                        key_char: None,
                    })
                    .collect();
                let first = &keys[0];
                if first.modifiers.platform || APP_KEYS.contains(&name(first).as_str()) {
                    continue;
                }
                let (bindings, pending) = keymap.bindings_for_input(&keys[..1], &terminal);
                let names: Vec<_> = bindings.iter().map(|b| b.action().name()).collect();
                assert!(
                    names.is_empty() && !pending,
                    "{} is taken from the terminal by {names:?}",
                    name(first)
                );
            }
        });
    }

    #[gpui_kit::test]
    fn app_shortcuts_still_work_from_the_terminal(cx: &mut TestAppContext) {
        cx.update(init);
        let workspace = stack(&["Root", "Workspace"]);
        let terminal = stack(&["Root", "Workspace", "Terminal"]);
        cx.update(|cx| {
            let keymap = cx.key_bindings();
            let keymap = keymap.borrow();
            for binding in keymap.bindings() {
                let keys: Vec<Keystroke> = binding
                    .keystrokes()
                    .iter()
                    .map(|k| Keystroke {
                        modifiers: *k.modifiers(),
                        key: k.key().to_string(),
                        key_char: None,
                    })
                    .collect();
                let reclaimed = super::bindings().iter().any(|b| {
                    b.keystrokes().len() == 1
                        && b.keystrokes()[0].key() == keys[0].key
                        && *b.keystrokes()[0].modifiers() == keys[0].modifiers
                });
                if reclaimed || binding.predicate().is_none_or(|p| !p.eval(&workspace)) {
                    continue;
                }
                let action = |stack| {
                    keymap
                        .bindings_for_input(&keys, stack)
                        .0
                        .first()
                        .map(|b| b.action().name())
                };
                assert_eq!(
                    action(&terminal),
                    action(&workspace),
                    "{:?} acts differently in the terminal",
                    keys.iter().map(name).collect::<Vec<_>>()
                );
            }
        });
    }

    /// Stand-in for the workspace: a tab stop beside a focused element with
    /// the terminal's key context and an `on_key_down` like `TerminalView`'s.
    struct Host {
        terminal: FocusHandle,
        received: Vec<String>,
    }

    impl Render for Host {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            div()
                .key_context("Workspace")
                .size_full()
                .child(
                    div()
                        .id("button")
                        .track_focus(&cx.focus_handle().tab_stop(true))
                        .child("0/0 instances running"),
                )
                .child(
                    div()
                        .id("terminal")
                        .key_context("Terminal")
                        .track_focus(&self.terminal)
                        .size_full()
                        .on_key_down(cx.listener(|this, e: &KeyDownEvent, _, cx| {
                            this.received.push(name(&e.keystroke));
                            cx.stop_propagation();
                        })),
                )
        }
    }

    #[gpui_kit::test]
    fn tab_stays_in_the_terminal(cx: &mut TestAppContext) {
        cx.update(init);
        let (window, host) = cx.update(|cx| {
            gpui_kit::open_window(Default::default(), cx, |_, cx| {
                cx.new(|cx| Host {
                    terminal: cx.focus_handle(),
                    received: Vec::new(),
                })
            })
            .unwrap()
        });
        let focus = cx.update(|cx| host.read(cx).terminal.clone());
        cx.update_window(window, |_, window, cx| window.focus(&focus, cx))
            .unwrap();
        cx.run_until_parked();

        let keys = "tab tab shift-tab cmd-c ctrl-r escape enter";
        cx.simulate_keystrokes(window, keys);

        let focused = cx
            .update_window(window, |_, window, _| focus.is_focused(window))
            .unwrap();
        assert!(focused, "focus left the terminal");
        let received = cx.update(|cx| host.read(cx).received.clone());
        assert_eq!(received, keys.split(' ').collect::<Vec<_>>());
    }

    #[test]
    fn right_clicks_open_the_menu_unless_sent_to_the_pane() {
        use super::{RightClick::*, right_click_target};
        assert!(matches!(right_click_target(false, false, true), Menu));
        assert!(matches!(right_click_target(true, false, true), Program));
        assert!(matches!(right_click_target(true, false, false), Nothing));
        assert!(matches!(right_click_target(true, true, true), Menu));
    }
}
