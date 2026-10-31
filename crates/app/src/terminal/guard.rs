//! Asking before Sawit ends terminals that still run something. Until
//! terminals outlive the app (the background sessions of the split-panes
//! spec), quitting, Restart to Update and closing a window end every process
//! in their terminals. So they ask first while one runs a command or an SSH
//! session, and say which; shells waiting at their prompt don't count.

use gpui_kit::*;

use super::TerminalPanel;

/// What would end the terminals.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Ending {
    Quit,
    /// Restart to Update: the app quits, and the new version opens.
    Restart,
    CloseWindow,
}

impl Ending {
    fn question(self) -> &'static str {
        match self {
            Ending::Quit => "Quit Sawit?",
            Ending::Restart => "Restart Sawit to update?",
            Ending::CloseWindow => "Close this window?",
        }
    }

    /// The button that goes on.
    fn answer(self) -> &'static str {
        match self {
            Ending::Quit => "Quit",
            Ending::Restart => "Restart",
            Ending::CloseWindow => "Close Window",
        }
    }

    fn doing(self) -> &'static str {
        match self {
            Ending::Quit => "Quitting",
            Ending::Restart => "Restarting",
            Ending::CloseWindow => "Closing the window",
        }
    }
}

/// The question's detail: what still runs, and that it would end.
pub fn detail(running: &[String], ending: Ending) -> String {
    const SHOWN: usize = 4;
    let mut names = running
        .iter()
        .take(SHOWN)
        .map(String::as_str)
        .collect::<Vec<_>>()
        .join(", ");
    if running.len() > SHOWN {
        names.push_str(&format!(" and {} more", running.len() - SHOWN));
    }
    let (what, them) = match running.len() {
        1 => ("A terminal is".to_string(), "it"),
        n => (format!("{n} terminals are"), "them"),
    };
    format!(
        "{what} still running: {names}. {} stops {them}.",
        ending.doing()
    )
}

/// Set while a question is up, so a second ⌘Q or close doesn't stack another.
#[derive(Default)]
struct Asking(bool);

impl Global for Asking {}

fn asking(cx: &App) -> bool {
    cx.try_global::<Asking>().is_some_and(|a| a.0)
}

fn set_asking(on: bool, cx: &mut App) {
    cx.default_global::<Asking>().0 = on;
}

/// Puts the question in `window`; true when the user goes on.
fn ask(
    running: &[String],
    ending: Ending,
    window: &mut Window,
    cx: &mut App,
) -> impl Future<Output = bool> + use<> {
    let answer = window.prompt(
        PromptLevel::Warning,
        ending.question(),
        Some(&detail(running, ending)),
        &[ending.answer(), "Cancel"],
        cx,
    );
    async move { matches!(answer.await, Ok(0)) }
}

/// Runs `then` (which quits or restarts the app) when no terminal of
/// `windows` runs anything, else once the user says so, asked in the first
/// window with something running (callers pass windows front to back).
/// Call it outside any window's update, which the question needs.
pub fn end_all(
    windows: Vec<(AnyWindowHandle, Entity<TerminalPanel>)>,
    ending: Ending,
    cx: &mut App,
    then: impl FnOnce(&mut App) + 'static,
) {
    let mut running = Vec::new();
    let mut ask_in = None;
    for (handle, panel) in &windows {
        let names = panel.read(cx).running(cx);
        if !names.is_empty() && ask_in.is_none() {
            ask_in = Some(*handle);
        }
        running.extend(names);
    }
    let Some(handle) = ask_in else {
        then(cx);
        return;
    };
    if asking(cx) {
        return;
    }
    let Ok(answer) = handle.update(cx, |_, window, cx| ask(&running, ending, window, cx)) else {
        return;
    };
    set_asking(true, cx);
    cx.spawn(async move |cx| {
        let go = answer.await;
        cx.update(|cx| {
            set_asking(false, cx);
            if go {
                then(cx);
            }
        });
    })
    .detach();
}

/// Makes closing `window` ask first while `panel`'s terminals run
/// something; "Close Window" then closes it.
pub fn ask_before_closing(window: &mut Window, panel: WeakEntity<TerminalPanel>, cx: &App) {
    window.on_window_should_close(cx, move |window, cx| {
        let Some(panel) = panel.upgrade() else {
            return true;
        };
        let running = panel.read(cx).running(cx);
        if running.is_empty() {
            return true;
        }
        if asking(cx) {
            return false;
        }
        set_asking(true, cx);
        let answer = ask(&running, Ending::CloseWindow, window, cx);
        let handle = window.window_handle();
        cx.spawn(async move |cx| {
            let close = answer.await;
            cx.update(|cx| {
                set_asking(false, cx);
                if close {
                    let _ = handle.update(cx, |_, window, _| window.remove_window());
                }
            });
        })
        .detach();
        false
    });
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::rc::Rc;

    use gpui_kit::{AnyWindowHandle, AppContext as _, Entity, TestAppContext, VisualTestContext};

    use super::super::TerminalPanel;
    use super::super::testing::{init, open_panel, ready, start};
    use super::{Ending, ask_before_closing, detail, end_all};

    #[test]
    fn the_question_names_what_would_end() {
        assert_eq!(
            detail(&["claude".into()], Ending::Quit),
            "A terminal is still running: claude. Quitting stops it."
        );
        let many = ["claude", "dev", "prod (SSH)", "vim", "top", "htop"].map(String::from);
        assert_eq!(
            detail(&many, Ending::CloseWindow),
            "6 terminals are still running: claude, dev, prod (SSH), vim and 2 more. Closing the \
             window stops them."
        );
    }

    /// Runs `sleep 30` in the first tab's shell, once it is ready, and waits until it runs.
    fn start_sleep(cx: &mut TestAppContext, panel: &Entity<TerminalPanel>) {
        let view = cx.update(|cx| panel.read(cx).tab(0).read(cx).focused_view().clone());
        ready(cx, &view);
        start(cx, &view, "sleep 30", "sleep");
    }

    /// Counts how often `end_all` went on (quit), asked about `panel`'s window.
    fn try_quit(
        cx: &mut TestAppContext,
        w: AnyWindowHandle,
        panel: &Entity<TerminalPanel>,
        quits: &Rc<Cell<u32>>,
    ) {
        let quits = quits.clone();
        let windows = vec![(w, panel.clone())];
        cx.update(|cx| {
            end_all(windows, Ending::Quit, cx, move |_| {
                quits.set(quits.get() + 1)
            })
        });
        cx.run_until_parked();
    }

    /// Quitting goes ahead at once while the shells wait at their prompts, and asks first while
    /// one runs something, naming it: Cancel keeps the app, Quit quits.
    #[gpui_kit::test]
    fn quitting_asks_while_a_terminal_runs_something(cx: &mut TestAppContext) {
        cx.update(init);
        let (w, panel) = open_panel(cx);
        let quits = Rc::new(Cell::new(0));
        let shell = cx.update(|cx| panel.read(cx).tab(0).read(cx).focused_view().clone());
        ready(cx, &shell);
        try_quit(cx, w, &panel, &quits);
        assert_eq!(quits.get(), 1, "an idle shell held up quitting");
        assert!(!cx.has_pending_prompt());

        start_sleep(cx, &panel);
        try_quit(cx, w, &panel, &quits);
        assert_eq!(quits.get(), 1, "quitting didn't ask");
        assert_eq!(
            cx.pending_prompt(),
            Some((
                "Quit Sawit?".to_string(),
                "A terminal is still running: sleep. Quitting stops it.".to_string()
            ))
        );
        cx.simulate_prompt_answer("Cancel");
        cx.run_until_parked();
        assert_eq!(quits.get(), 1, "Cancel quit");

        try_quit(cx, w, &panel, &quits);
        cx.simulate_prompt_answer("Quit");
        cx.run_until_parked();
        assert_eq!(quits.get(), 2);
    }

    /// Closing a window asks first while its terminals run something; Cancel keeps it open and
    /// Close Window closes it.
    #[gpui_kit::test]
    fn closing_a_window_asks_while_a_terminal_runs_something(cx: &mut TestAppContext) {
        cx.update(init);
        let (w, panel) = open_panel(cx);
        cx.update_window(w, |_, window, cx| {
            ask_before_closing(window, panel.downgrade(), cx)
        })
        .expect("the test window is open");
        let shell = cx.update(|cx| panel.read(cx).tab(0).read(cx).focused_view().clone());
        ready(cx, &shell);
        let mut vcx = VisualTestContext::from_window(w, cx);
        assert!(vcx.simulate_close(), "a window of idle shells asked");
        assert!(!cx.has_pending_prompt());

        start_sleep(cx, &panel);
        let mut vcx = VisualTestContext::from_window(w, cx);
        assert!(!vcx.simulate_close(), "the window closed without asking");
        let (question, _) = cx.pending_prompt().expect("a question");
        assert_eq!(question, "Close this window?");
        cx.simulate_prompt_answer("Cancel");
        cx.run_until_parked();
        assert!(cx.windows().contains(&w), "Cancel closed the window");

        let mut vcx = VisualTestContext::from_window(w, cx);
        assert!(!vcx.simulate_close());
        cx.simulate_prompt_answer("Close Window");
        cx.run_until_parked();
        assert!(!cx.windows().contains(&w), "the window is still open");
    }
}
