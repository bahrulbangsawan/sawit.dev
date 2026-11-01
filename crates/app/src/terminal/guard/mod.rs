//! Asking before Sawit ends terminals that still run something, or closes files with unsaved
//! changes. Until terminals outlive the app (the background sessions of the split-panes spec),
//! quitting, Restart to Update and closing a window end every process in their terminals. So they
//! ask first while one runs a command or an SSH session, and say which; shells waiting at their
//! prompt don't count. They close every file too, so they also ask to save the files with unsaved
//! changes first (SITE-1), and they stop what the Dev Tools pages install: one question covers
//! all three.
//!
//! A quit that can't ask (the Dock's Quit, logging out, the install script's `quit app`) goes
//! straight through: the file editors then keep their unsaved changes for the next launch
//! ([`crate::editor::kept`]).

use gpui_kit::*;

use super::TerminalPanel;
use crate::editor::FileEditor;

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

    /// What "Save changes to … before" goes on with.
    fn before(self) -> &'static str {
        match self {
            Ending::Quit => "quitting",
            Ending::Restart => "restarting",
            Ending::CloseWindow => "closing this window",
        }
    }
}

/// `names` for a sentence: the first four, and how many more.
fn listed(names: &[String]) -> String {
    const SHOWN: usize = 4;
    let mut listed = names
        .iter()
        .take(SHOWN)
        .map(String::as_str)
        .collect::<Vec<_>>()
        .join(", ");
    if names.len() > SHOWN {
        listed.push_str(&format!(" and {} more", names.len() - SHOWN));
    }
    listed
}

/// The question's detail: what still runs (the terminals, by name, and what the Dev Tools pages
/// do: `tools`, such as "Installing Bun, 2 more queued"), and that it would end.
pub fn detail(running: &[String], tools: &[String], ending: Ending) -> String {
    let mut detail = String::new();
    if !running.is_empty() {
        let what = match running.len() {
            1 => "A terminal is".to_string(),
            n => format!("{n} terminals are"),
        };
        detail.push_str(&format!("{what} still running: {}. ", listed(running)));
    }
    if !tools.is_empty() {
        detail.push_str(&format!(
            "Dev Tools is still running: {}. ",
            tools.join("; ")
        ));
    }
    let them = if running.len() + tools.len() == 1 {
        "it"
    } else {
        "them"
    };
    detail.push_str(&format!("{} stops {them}.", ending.doing()));
    detail
}

/// The question when files have unsaved changes: the file, or how many.
pub fn save_question(files: &[String], ending: Ending) -> String {
    let what = match files {
        [one] => one.clone(),
        _ => format!("{} files", files.len()),
    };
    format!("Save changes to {what} before {}?", ending.before())
}

/// Its detail: the files when there are several, that unsaved changes are lost, and what still
/// runs in the terminals and the Dev Tools pages.
pub fn save_detail(
    files: &[String],
    running: &[String],
    tools: &[String],
    ending: Ending,
) -> String {
    let mut detail = String::new();
    if files.len() > 1 {
        detail.push_str(&format!("{}. ", listed(files)));
    }
    detail.push_str("Your changes will be lost if you don't save them.");
    if !running.is_empty() || !tools.is_empty() {
        detail.push(' ');
        detail.push_str(&self::detail(running, tools, ending));
    }
    detail
}

/// What the user answered.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Answer {
    /// Cancel.
    Stay,
    /// Quit, Restart, Close Window, or Don't Save.
    Go,
    /// Save (or Save All), then go on.
    SaveAndGo,
}

/// The window whose question is up, so a second ⌘Q or close doesn't stack another question but
/// brings that window forward: the user may have gone to another window since.
#[derive(Default)]
struct Asking(Option<AnyWindowHandle>);

impl Global for Asking {}

/// While a question is up, brings its window forward and answers true. `here` is the window
/// being updated, if any, which can only be reached through itself. A question whose window has
/// gone counts as answered.
fn question_up(here: Option<&mut Window>, cx: &mut App) -> bool {
    let Some(asked) = cx.try_global::<Asking>().and_then(|a| a.0) else {
        return false;
    };
    match here {
        Some(window) if window.window_handle() == asked => {
            window.activate_window();
            true
        }
        _ => asked
            .update(cx, |_, window, _| window.activate_window())
            .is_ok(),
    }
}

/// Clears the question in `window`, unless another one has taken its place.
fn done_asking(window: AnyWindowHandle, cx: &mut App) {
    let asking = cx.default_global::<Asking>();
    if asking.0 == Some(window) {
        asking.0 = None;
    }
}

/// Puts the question in `window`, which comes forward with it: the window may be behind the one
/// the user works in (only it runs something), and a sheet there is never seen. With `unsaved`
/// files it asks to save them (Save, or Save All, Don't Save, Cancel), and says what still runs;
/// else only what runs: the terminals' `running` and the Dev Tools pages' `tools`.
fn ask(
    running: &[String],
    tools: &[String],
    unsaved: &[Entity<FileEditor>],
    ending: Ending,
    window: &mut Window,
    cx: &mut App,
) -> impl Future<Output = Answer> + use<> {
    cx.default_global::<Asking>().0 = Some(window.window_handle());
    window.activate_window();
    let files: Vec<String> = unsaved.iter().map(|e| e.read(cx).title()).collect();
    let saving = !files.is_empty();
    let (question, detail, answers) = if saving {
        let save = if files.len() == 1 { "Save" } else { "Save All" };
        (
            save_question(&files, ending),
            save_detail(&files, running, tools, ending),
            vec![save, "Don't Save", "Cancel"],
        )
    } else {
        (
            ending.question().to_string(),
            detail(running, tools, ending),
            vec![ending.answer(), "Cancel"],
        )
    };
    let answer = window.prompt(PromptLevel::Warning, &question, Some(&detail), &answers, cx);
    async move {
        match (answer.await, saving) {
            (Ok(0), true) => Answer::SaveAndGo,
            (Ok(0), false) | (Ok(1), true) => Answer::Go,
            _ => Answer::Stay,
        }
    }
}

/// Does what the user answered with the `unsaved` files, and says whether the ending goes on.
/// Save saves them all first, and goes on only once every one is saved: one that couldn't be is
/// still open, its notice saying why. Don't Save goes on, and the quit doesn't keep their changes.
async fn settle(answer: Answer, unsaved: &[Entity<FileEditor>], cx: &mut AsyncApp) -> bool {
    match answer {
        Answer::Stay => false,
        Answer::Go => {
            cx.update(|cx| {
                for editor in unsaved {
                    editor.update(cx, |e, _| e.discard());
                }
            });
            true
        }
        Answer::SaveAndGo => {
            let saves: Vec<Task<bool>> = cx.update(|cx| {
                unsaved
                    .iter()
                    .map(|editor| editor.update(cx, |e, cx| e.save_task(cx)))
                    .collect()
            });
            let mut saved = true;
            for save in saves {
                saved &= save.await;
            }
            saved
        }
    }
}

/// Runs `then` (which quits or restarts the app) when no terminal of
/// `windows` runs anything, no file has unsaved changes and no Dev Tools page
/// installs anything, else once the user says so (and the files they chose to
/// save are saved), asked in the first window with something running or
/// unsaved (callers pass windows front to back), which comes forward with the
/// question. Call it outside any window's update, which the question needs.
pub fn end_all(
    windows: Vec<(AnyWindowHandle, Entity<TerminalPanel>)>,
    ending: Ending,
    cx: &mut App,
    then: impl FnOnce(&mut App) + 'static,
) {
    let unsaved = crate::editor::unsaved(cx);
    let mut installs = crate::tools_page::at_work(cx);
    installs.extend(crate::skills_page::at_work(cx));
    installs.extend(crate::new_project_page::at_work(cx));
    let mut running = Vec::new();
    let mut ask_in = None;
    for (handle, panel) in &windows {
        let names = panel.read(cx).running(cx);
        let has_unsaved = unsaved.iter().any(|(window, _)| window == handle);
        let installing = installs.iter().any(|(window, _)| window == handle);
        if (!names.is_empty() || has_unsaved || installing) && ask_in.is_none() {
            ask_in = Some(*handle);
        }
        running.extend(names);
    }
    // Every Sawit window has a terminal dock; a window without one asks for its own files and
    // installs.
    let ask_in = ask_in
        .or_else(|| unsaved.first().map(|(window, _)| *window))
        .or_else(|| installs.first().map(|(window, _)| *window));
    let Some(handle) = ask_in else {
        then(cx);
        return;
    };
    if question_up(None, cx) {
        return;
    }
    let files: Vec<Entity<FileEditor>> = unsaved.into_iter().map(|(_, editor)| editor).collect();
    let tools: Vec<String> = installs.into_iter().map(|(_, work)| work).collect();
    let Ok(answer) = handle.update(cx, |_, window, cx| {
        ask(&running, &tools, &files, ending, window, cx)
    }) else {
        return;
    };
    cx.spawn(async move |cx| {
        let go = settle(answer.await, &files, cx).await;
        cx.update(|cx| {
            done_asking(handle, cx);
            if go {
                then(cx);
            }
        });
    })
    .detach();
}

/// Makes closing `window` ask first while `panel`'s terminals run
/// something, a file in the window has unsaved changes or its Dev Tools page
/// installs something (closing the window stops that, as closing the tab
/// does); "Close Window", Don't Save, or Save once the files are saved, then
/// closes it.
///
/// A window has one should-close hook: a later `on_window_should_close`
/// replaces this one. Anything else that must ask before the window closes
/// belongs in this question.
pub fn ask_before_closing(window: &mut Window, panel: WeakEntity<TerminalPanel>, cx: &App) {
    window.on_window_should_close(cx, move |window, cx| {
        let handle = window.window_handle();
        let running = panel
            .upgrade()
            .map(|panel| panel.read(cx).running(cx))
            .unwrap_or_default();
        let files: Vec<Entity<FileEditor>> = crate::editor::unsaved(cx)
            .into_iter()
            .filter(|(window, _)| *window == handle)
            .map(|(_, editor)| editor)
            .collect();
        let tools: Vec<String> = crate::tools_page::at_work(cx)
            .into_iter()
            .chain(crate::skills_page::at_work(cx))
            .chain(crate::new_project_page::at_work(cx))
            .filter(|(window, _)| *window == handle)
            .map(|(_, work)| work)
            .collect();
        if running.is_empty() && files.is_empty() && tools.is_empty() {
            return true;
        }
        if question_up(Some(&mut *window), cx) {
            return false;
        }
        let answer = ask(&running, &tools, &files, Ending::CloseWindow, window, cx);
        cx.spawn(async move |cx| {
            let close = settle(answer.await, &files, cx).await;
            cx.update(|cx| {
                done_asking(handle, cx);
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
mod tests;
