//! One terminal: draws `core::terminal` snapshots on a canvas (like Zed's
//! terminal element) and forwards keyboard, mouse and paste input. Dragging
//! selects text and copies it on release; Shift+drag selects even while a
//! program (vim, Claude Code) has the mouse. Dropped files are typed as paths.

mod agent;
mod keys;
mod mouse;
mod paint;
mod render;

use std::cell::Cell;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::{Duration, Instant};

use futures::StreamExt;
use gpui_kit::*;
use sawit_core::terminal::{
    AgentKind, MouseButton as TermButton, Palette, ProcessArgs, ScrollAccumulator, Session,
    TermEvent,
};

pub(super) use keys::bindings;
pub(super) use paint::cell_size;
use paint::hsla;

/// How long the "Copied to clipboard" toast stays up.
const TOAST: Duration = Duration::from_millis(1400);
/// Space left and right of the grid.
pub(super) const PAD_X: Pixels = px(8.);
/// How long a pane header shows a folder before asking the OS again.
pub(super) const FOLDER_TTL: Duration = Duration::from_millis(500);
/// How long after output (or a new title) a pane looks again at which coding agent runs in it:
/// a job that starts or ends draws something, and this is soon after, while a busy program's
/// stream of output gets one look per interval, not one per frame.
pub(super) const AGENT_CHECK: Duration = Duration::from_millis(300);

/// Grid origin and cell size on screen.
type GridLayout = (Point<Pixels>, Size<Pixels>);

pub enum TerminalEvent {
    TitleChanged,
    /// A coding agent started or ended in the foreground ([`TerminalView::agent`]).
    AgentChanged,
    Exited,
    /// A right-click asked for the pane menu, at this window position.
    MenuRequested(Point<Pixels>),
    /// The terminal took the keyboard without a click on its pane (files
    /// were dropped on it), so its tab should focus its pane.
    Focused,
}

impl EventEmitter<TerminalEvent> for TerminalView {}

pub struct TerminalView {
    session: Rc<Session>,
    palette: Rc<Palette>,
    focus: FocusHandle,
    pub title: SharedString,
    pub exited: bool,
    /// Runs the user's shell, so commands can be typed into it.
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
    /// Asks for the folder again a moment after the header last read it,
    /// redrawing if it changed.
    folder_refresh: Option<Task<()>>,
    /// The coding agent running in the foreground, as last looked at.
    agent: Option<AgentKind>,
    /// The foreground job last looked at, by its process group and the name of its program,
    /// with that program's executable and arguments: read again only when either changes (the
    /// shell's fork becoming the program it runs changes the name).
    agent_job: Option<(ForegroundKey, Option<ProcessArgs>)>,
    /// The foreground job when the program last set the title: the title is that job's own.
    /// `None` until a program sets one.
    title_job: Option<Option<u32>>,
    /// A look for the agent, due [`AGENT_CHECK`] after the output that asked for it.
    agent_check: Option<Task<()>>,
    /// When the pane last drew with the keyboard in it.
    keyboard_at: Option<Instant>,
    /// Stands in for what runs in the foreground (for tests).
    #[cfg(test)]
    fake_agent: Option<Option<AgentKind>>,
    _events: Task<()>,
}

/// A foreground job and the name of the program it runs.
type ForegroundKey = (u32, Option<String>);

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
                                    this.title_job = Some(this.session.foreground_job());
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
                    this.check_agent_soon(cx);
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
            agent: None,
            agent_job: None,
            title_job: None,
            agent_check: None,
            keyboard_at: None,
            #[cfg(test)]
            fake_agent: None,
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
    /// since headers redraw with every frame of output. Each read also
    /// schedules one more look 500 ms later, so after a `cd` the header
    /// settles on the new folder even if no more output comes. That look
    /// redraws only when the folder changed, so an idle pane stays idle.
    pub fn folder(&mut self, cx: &mut Context<Self>) -> Option<PathBuf> {
        let dir = match &self.folder {
            Some((at, dir)) if at.elapsed() < FOLDER_TTL => dir.clone(),
            _ => {
                let dir = self.cwd();
                self.folder = Some((Instant::now(), dir.clone()));
                dir
            }
        };
        if self.folder_refresh.is_none() {
            self.folder_refresh = Some(cx.spawn(async move |this, cx| {
                cx.background_executor().timer(FOLDER_TTL).await;
                let _ = this.update(cx, |this, cx| {
                    this.folder_refresh = None;
                    let dir = this.cwd();
                    let changed = this.folder.as_ref().is_none_or(|(_, old)| *old != dir);
                    this.folder = Some((Instant::now(), dir));
                    if changed {
                        cx.notify();
                    }
                });
            }));
        }
        dir
    }

    /// The folder the header shows now, without asking the OS (for tests).
    #[cfg(test)]
    pub(super) fn cached_folder(&self) -> Option<PathBuf> {
        self.folder.as_ref().and_then(|(_, dir)| dir.clone())
    }

    /// The grid size the terminal was last laid out at (for tests).
    #[cfg(test)]
    pub(super) fn grid(&self) -> sawit_core::terminal::GridSize {
        self.session.size()
    }

    /// The shell is waiting at its prompt, not running a command.
    pub fn at_prompt(&self) -> bool {
        self.shell && !self.exited && self.session.at_prompt()
    }

    /// The terminal's background colour.
    pub fn background(&self) -> Hsla {
        hsla(self.palette.background)
    }

    /// Nothing here would take a key the panes use (⇧⇥): the shell waits at its prompt with no
    /// program in the foreground (what the PTY's foreground process group says, read now), or
    /// the process has ended. An SSH session's remote shell can't be seen from here, so the
    /// session always takes the key.
    pub fn nothing_in_front(&self) -> bool {
        self.exited || self.at_prompt()
    }

    /// A plain shell waiting at its prompt: what commands from elsewhere (an agent's, a
    /// preview's Run) may be typed into, and what follows the first project's folder. A Run
    /// panel script's terminal is a shell too, waiting at its prompt between runs, but it
    /// stays the script's: what runs there is read as the script running.
    pub fn idle_shell(&self) -> bool {
        self.at_prompt() && self.remote.is_none() && self.job.is_none()
    }

    /// The command the shell is running, by process group.
    pub fn foreground_job(&self) -> Option<u32> {
        if self.exited {
            return None;
        }
        self.session.foreground_job()
    }

    /// The name of the command the shell is running (`vim`, `claude`).
    pub fn foreground_name(&self) -> Option<String> {
        if self.exited {
            return None;
        }
        self.session.foreground_name()
    }

    /// When the pane last drew with the keyboard in it: the latest of all panes is where the
    /// user types, or last typed.
    pub fn keyboard_at(&self) -> Option<Instant> {
        self.keyboard_at
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
}

impl Focusable for TerminalView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

#[cfg(test)]
mod tests;
