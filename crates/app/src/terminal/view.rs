//! One terminal: draws `core::terminal` snapshots on a canvas (like Zed's
//! terminal element) and forwards keyboard, mouse and paste input.

use std::cell::Cell;
use std::rc::Rc;

use futures::StreamExt;
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::*;
use pgdesk_core::terminal::{
    CursorView, GridSize, KeyInput, MouseAction, MouseButton as TermButton, Palette, Rgb,
    ScrollAccumulator, Session, Snapshot, TermEvent, key_to_bytes, mouse_report, paste_bytes,
};

const FONT_SIZE: f32 = 13.;
const LINE_HEIGHT: f32 = 1.3;

/// Grid origin and cell size on screen.
type GridLayout = (Point<Pixels>, Size<Pixels>);

pub enum TerminalEvent {
    TitleChanged,
    Exited,
}

impl EventEmitter<TerminalEvent> for TerminalView {}

pub struct TerminalView {
    session: Rc<Session>,
    palette: Rc<Palette>,
    focus: FocusHandle,
    pub title: SharedString,
    pub exited: bool,
    /// Grid origin and cell size from the last paint, for mouse mapping.
    layout: Rc<Cell<Option<GridLayout>>>,
    pressed: Option<TermButton>,
    scroll: ScrollAccumulator,
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
                            TermEvent::Copy(text) => {
                                cx.write_to_clipboard(ClipboardItem::new_string(text))
                            }
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
            layout: Rc::new(Cell::new(None)),
            pressed: None,
            scroll: ScrollAccumulator::default(),
            _events: task,
        }
    }

    pub fn focus(&self, window: &mut Window, cx: &mut App) {
        window.focus(&self.focus, cx);
    }

    fn modes(&self) -> pgdesk_core::terminal::Modes {
        self.session.modes()
    }

    fn on_key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let ks = &event.keystroke;
        let m = &ks.modifiers;
        if m.platform && ks.key == "v" {
            if let Some(text) = cx.read_from_clipboard().and_then(|c| c.text()) {
                self.session.write(paste_bytes(&text, self.modes()));
                self.session.scroll_to_bottom();
            }
            cx.stop_propagation();
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
            self.session.write(bytes);
            self.session.scroll_to_bottom();
            cx.stop_propagation();
            window.prevent_default();
        }
    }

    /// Grid cell under `position`, clamped to the screen.
    fn cell_at(&self, position: Point<Pixels>) -> Option<(usize, usize)> {
        let (origin, cell) = self.layout.get()?;
        let size = self.session.size();
        let col = ((position.x - origin.x) / cell.width).floor().max(0.) as usize;
        let row = ((position.y - origin.y) / cell.height).floor().max(0.) as usize;
        Some((
            col.min(size.cols as usize - 1),
            row.min(size.rows as usize - 1),
        ))
    }

    fn send_mouse(&self, action: MouseAction, position: Point<Pixels>, shift: bool) -> bool {
        let Some((col, row)) = self.cell_at(position) else {
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
}

/// Paints one snapshot: backgrounds, text runs (glyphs forced onto the cell
/// grid), then the cursor.
fn paint_snapshot(snap: &Snapshot, p: &Painter, window: &mut Window, cx: &mut App) {
    let (palette, origin, cell, font, focused) = (p.palette, p.origin, p.cell, p.font, p.focused);

    let font_size = px(FONT_SIZE);
    for (row, line) in snap.lines.iter().enumerate() {
        let y = origin.y + cell.height * row as f32;
        // Backgrounds, merged across equal neighbours.
        let mut col = 0;
        while col < line.len() {
            let bg = line[col].bg;
            let start = col;
            while col < line.len() && line[col].bg == bg {
                col += 1;
            }
            if bg != palette.background {
                let bounds = Bounds::new(
                    point(origin.x + cell.width * start as f32, y),
                    size(cell.width * (col - start) as f32, cell.height),
                );
                window.paint_quad(fill(bounds, hsla(bg)));
            }
        }

        // Text: one shaped line per row, a run per style change.
        let mut text = String::with_capacity(line.len());
        let mut runs: Vec<TextRun> = Vec::new();
        for c in line {
            let ch = if c.spacer { ' ' } else { c.ch };
            let len = ch.len_utf8();
            text.push(ch);
            let mut run_font = font.clone();
            if c.bold {
                run_font.weight = FontWeight::BOLD;
            }
            if c.italic {
                run_font.style = FontStyle::Italic;
            }
            let color = hsla(c.fg);
            let underline = c.underline.then(|| UnderlineStyle {
                thickness: px(1.),
                color: Some(color),
                wavy: false,
            });
            let strikethrough = c.strikeout.then(|| StrikethroughStyle {
                thickness: px(1.),
                color: Some(color),
            });
            match runs.last_mut() {
                Some(last)
                    if last.font == run_font
                        && last.color == color
                        && last.underline == underline
                        && last.strikethrough == strikethrough =>
                {
                    last.len += len
                }
                _ => runs.push(TextRun {
                    len,
                    font: run_font,
                    color,
                    background_color: None,
                    underline,
                    strikethrough,
                }),
            }
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
        let font_id = window.text_system().resolve_font(&font);
        let font_size = px(FONT_SIZE);
        let cell_width = window
            .text_system()
            .advance(font_id, font_size, 'm')
            .map(|s| s.width)
            .unwrap_or(px(8.));
        let cell = size(cell_width, (font_size * LINE_HEIGHT).round());
        let session = self.session.clone();
        let palette = self.palette.clone();
        let layout = self.layout.clone();
        let focused = self.focus.is_focused(window);
        let exited = self.exited;

        div()
            .id("terminal")
            .key_context("Terminal")
            .track_focus(&self.focus)
            .size_full()
            .px_2()
            .bg(hsla(self.palette.background))
            .on_key_down(cx.listener(Self::on_key))
            .on_scroll(cx.listener(Self::on_scroll))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, e: &MouseDownEvent, window, cx| {
                    this.focus(window, cx);
                    if this.send_mouse(
                        MouseAction::Press(TermButton::Left),
                        e.position,
                        e.modifiers.shift,
                    ) {
                        this.pressed = Some(TermButton::Left);
                    }
                }),
            )
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(|this, e: &MouseDownEvent, _, _| {
                    if this.send_mouse(
                        MouseAction::Press(TermButton::Right),
                        e.position,
                        e.modifiers.shift,
                    ) {
                        this.pressed = Some(TermButton::Right);
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
                        let snap = session.snapshot(&palette);
                        window.paint_quad(fill(bounds, hsla(palette.background)));
                        let painter = Painter {
                            palette: &palette,
                            origin: bounds.origin,
                            cell,
                            font: &font,
                            focused,
                        };
                        paint_snapshot(&snap, &painter, window, cx);
                        if exited {
                            let note = "[proses selesai — tutup tab ini atau buka terminal baru]";
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
    }
}

impl Focusable for TerminalView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}
