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
    let installs = crate::tools_page::at_work(cx);
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
mod tests {
    use std::cell::Cell;
    use std::path::{Path, PathBuf};
    use std::rc::Rc;

    use gpui_kit::{
        AnyWindowHandle, AppContext as _, Entity, TestAppContext, VisualTestContext, WeakEntity,
    };

    use super::super::TerminalPanel;
    use super::super::testing::{init, open_panel, ready, start};
    use super::{Ending, ask_before_closing, detail, end_all, save_detail, save_question};
    use crate::editor::FileEditor;
    use crate::editor::testing::{open_editor, scratch_dir, type_into, wait_until};
    use crate::tools_page::ToolsPage;

    #[test]
    fn the_question_names_what_would_end() {
        assert_eq!(
            detail(&["claude".into()], &[], Ending::Quit),
            "A terminal is still running: claude. Quitting stops it."
        );
        let many = ["claude", "dev", "prod (SSH)", "vim", "top", "htop"].map(String::from);
        assert_eq!(
            detail(&many, &[], Ending::CloseWindow),
            "6 terminals are still running: claude, dev, prod (SSH), vim and 2 more. Closing the \
             window stops them."
        );
        // What the Dev Tools pages run, as each page says it (one page per window).
        let tools = ["Installing Bun, 2 more queued", "Upgrading uv"].map(String::from);
        assert_eq!(
            detail(&[], &tools[..1], Ending::Restart),
            "Dev Tools is still running: Installing Bun, 2 more queued. Restarting stops it."
        );
        assert_eq!(
            detail(&[], &tools, Ending::Quit),
            "Dev Tools is still running: Installing Bun, 2 more queued; Upgrading uv. Quitting \
             stops them."
        );
    }

    /// With files unsaved the question asks to save them, by name, or by count past one, then
    /// names them in its detail (the first four, and how many more).
    #[test]
    fn the_save_question_counts_and_names_the_files() {
        assert_eq!(
            save_question(&["notes.txt".into()], Ending::Restart),
            "Save changes to notes.txt before restarting?"
        );
        let files = ["a.rs", "b.rs", "c.rs", "d.rs", "e.rs"].map(String::from);
        assert_eq!(
            save_question(&files, Ending::CloseWindow),
            "Save changes to 5 files before closing this window?"
        );
        assert_eq!(
            save_detail(&files, &["dev".into(), "vim".into()], &[], Ending::Quit),
            "a.rs, b.rs, c.rs, d.rs and 1 more. Your changes will be lost if you don't save them. \
             2 terminals are still running: dev, vim. Quitting stops them."
        );
        assert_eq!(
            save_detail(
                &files[..1],
                &[],
                &["Installing Bun".into()],
                Ending::CloseWindow
            ),
            "Your changes will be lost if you don't save them. Dev Tools is still running: \
             Installing Bun. Closing the window stops it."
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

    /// The window the platform has in front.
    fn front(cx: &mut TestAppContext) -> Option<AnyWindowHandle> {
        cx.update(|cx| cx.active_window())
    }

    /// Brings `w` to the front, as a click on it does.
    fn bring_forward(cx: &mut TestAppContext, w: AnyWindowHandle) {
        cx.update_window(w, |_, window, _| window.activate_window())
            .expect("the test window is open");
        cx.run_until_parked();
    }

    /// With two windows, the question goes up in the one whose terminal runs something, behind
    /// the idle one in front, and that window comes forward with it: a question on a window
    /// behind is never seen, and ⌘Q seems to do nothing. ⌘Q again, from the front window, brings
    /// the question forward again instead of stacking a second one.
    #[gpui_kit::test]
    fn the_question_comes_forward_with_its_window(cx: &mut TestAppContext) {
        cx.update(init);
        let (idle_window, idle) = open_panel(cx);
        let (busy_window, busy) = open_panel(cx);
        let shell = cx.update(|cx| idle.read(cx).tab(0).read(cx).focused_view().clone());
        ready(cx, &shell);
        start_sleep(cx, &busy);
        bring_forward(cx, idle_window);
        assert_eq!(front(cx), Some(idle_window));

        let quits = Rc::new(Cell::new(0));
        let quit = |cx: &mut TestAppContext| {
            let quits = quits.clone();
            let windows = vec![(idle_window, idle.clone()), (busy_window, busy.clone())];
            cx.update(|cx| {
                end_all(windows, Ending::Quit, cx, move |_| {
                    quits.set(quits.get() + 1)
                })
            });
            cx.run_until_parked();
        };
        quit(cx);
        assert!(cx.has_pending_prompt(), "quitting didn't ask");
        assert_eq!(
            front(cx),
            Some(busy_window),
            "the question went up on the window behind"
        );

        bring_forward(cx, idle_window);
        quit(cx);
        assert_eq!(
            front(cx),
            Some(busy_window),
            "⌘Q again left the question behind"
        );
        cx.simulate_prompt_answer("Cancel");
        cx.run_until_parked();
        assert!(!cx.has_pending_prompt(), "a second question was stacked");
        assert_eq!(quits.get(), 0, "Cancel quit");
    }

    /// Closing a window while the quit question is up, this window or another, keeps it open
    /// and brings the question forward rather than stacking a second one.
    #[gpui_kit::test]
    fn closing_while_the_question_is_up_brings_it_forward(cx: &mut TestAppContext) {
        cx.update(init);
        let (asked, asked_panel) = open_panel(cx);
        let (other, other_panel) = open_panel(cx);
        for (w, panel) in [(asked, &asked_panel), (other, &other_panel)] {
            cx.update_window(w, |_, window, cx| {
                ask_before_closing(window, panel.downgrade(), cx)
            })
            .expect("the test window is open");
            start_sleep(cx, panel);
        }
        let windows = vec![(asked, asked_panel.clone()), (other, other_panel.clone())];
        cx.update(|cx| end_all(windows, Ending::Quit, cx, |_| {}));
        cx.run_until_parked();
        assert!(cx.has_pending_prompt(), "quitting didn't ask");

        for (w, which) in [(other, "another window"), (asked, "the asked window")] {
            bring_forward(cx, other);
            let mut vcx = VisualTestContext::from_window(w, cx);
            assert!(!vcx.simulate_close(), "closing {which} didn't wait");
            cx.run_until_parked();
            assert_eq!(
                front(cx),
                Some(asked),
                "closing {which} left the question behind"
            );
        }
        cx.simulate_prompt_answer("Cancel");
        cx.run_until_parked();
        assert!(!cx.has_pending_prompt(), "a second question was stacked");
        assert!(cx.windows().contains(&asked) && cx.windows().contains(&other));
    }

    /// A file with unsaved changes ("typed " before its "on disk\n"), `name` in a folder of its
    /// own, open in a window of its own.
    fn unsaved_file(
        cx: &mut TestAppContext,
        folder: &str,
        name: &str,
    ) -> (AnyWindowHandle, Entity<FileEditor>, PathBuf) {
        let path = scratch_dir(folder).join(name);
        std::fs::write(&path, "on disk\n").unwrap();
        let (w, editor) = open_editor(cx, &path);
        type_into(cx, w, &editor, "typed ");
        (w, editor, path)
    }

    fn on_disk(path: &Path) -> String {
        std::fs::read_to_string(path).unwrap()
    }

    /// Calls `end_all` for `ending` with no terminal docks, as ⌘Q or Restart to Update do in a
    /// window whose terminals run nothing, and counts in `ends` how often it went on.
    fn try_ending(cx: &mut TestAppContext, ending: Ending, ends: &Rc<Cell<u32>>) {
        let ends = ends.clone();
        cx.update(|cx| end_all(Vec::new(), ending, cx, move |_| ends.set(ends.get() + 1)));
        cx.run_until_parked();
    }

    /// SITE-1: quitting with a file's changes unsaved asks to save them first, as closing its tab
    /// does. Cancel keeps the app; Save writes the file, then quits; with nothing unsaved left,
    /// quitting goes ahead without asking. It used to quit at once, and the changes were lost.
    #[gpui_kit::test]
    fn quitting_asks_to_save_a_file_with_unsaved_changes(cx: &mut TestAppContext) {
        cx.update(crate::editor::testing::init);
        let (_, editor, path) = unsaved_file(cx, "quit-save", "notes.txt");
        let quits = Rc::new(Cell::new(0));

        try_ending(cx, Ending::Quit, &quits);
        assert_eq!(quits.get(), 0, "quitting didn't ask about the unsaved file");
        assert_eq!(
            cx.pending_prompt(),
            Some((
                "Save changes to notes.txt before quitting?".to_string(),
                "Your changes will be lost if you don't save them.".to_string()
            ))
        );
        cx.simulate_prompt_answer("Cancel");
        cx.run_until_parked();
        assert_eq!(quits.get(), 0, "Cancel quit");
        assert_eq!(on_disk(&path), "on disk\n");

        try_ending(cx, Ending::Quit, &quits);
        cx.simulate_prompt_answer("Save");
        wait_until(cx, "it saves, then quits", |_| quits.get() == 1);
        assert_eq!(on_disk(&path), "typed on disk\n");
        assert!(cx.update(|cx| !editor.read(cx).dirty));

        try_ending(cx, Ending::Quit, &quits);
        assert_eq!(quits.get(), 2, "a saved file held up quitting");
        assert!(!cx.has_pending_prompt());
    }

    /// With several unsaved files the question counts and names them and offers Save All, which
    /// goes on only once every one is saved: a file that can't be saved (here its tab was made
    /// read-only) keeps the app, its notice saying why. Don't Save restarts and writes nothing.
    #[gpui_kit::test]
    fn ending_with_several_unsaved_files_saves_them_all_or_stays(cx: &mut TestAppContext) {
        cx.update(crate::editor::testing::init);
        let (_, notes, notes_path) = unsaved_file(cx, "many-a", "notes.txt");
        let (_, todo, todo_path) = unsaved_file(cx, "many-b", "todo.md");
        let restarts = Rc::new(Cell::new(0));

        try_ending(cx, Ending::Restart, &restarts);
        assert_eq!(
            cx.pending_prompt(),
            Some((
                "Save changes to 2 files before restarting?".to_string(),
                "notes.txt, todo.md. Your changes will be lost if you don't save them.".to_string()
            ))
        );
        cx.update(|cx| todo.update(cx, |e, cx| e.set_read_only(true, cx)));
        cx.simulate_prompt_answer("Save All");
        wait_until(cx, "notes.txt is saved", |cx| !notes.read(cx).dirty);
        cx.run_until_parked();
        assert_eq!(
            restarts.get(),
            0,
            "restarted with a file that couldn't be saved"
        );
        assert_eq!(on_disk(&notes_path), "typed on disk\n");
        assert_eq!(on_disk(&todo_path), "on disk\n");

        try_ending(cx, Ending::Restart, &restarts);
        let (question, _) = cx.pending_prompt().expect("a question");
        assert_eq!(question, "Save changes to todo.md before restarting?");
        cx.simulate_prompt_answer("Don't Save");
        cx.run_until_parked();
        assert_eq!(restarts.get(), 1, "Don't Save didn't restart");
        assert_eq!(on_disk(&todo_path), "on disk\n");
    }

    /// One question covers both: a terminal that runs something and a file with unsaved changes.
    #[gpui_kit::test]
    fn quitting_asks_once_about_a_running_terminal_and_an_unsaved_file(cx: &mut TestAppContext) {
        cx.update(|cx| {
            crate::editor::testing::init(cx);
            init(cx);
        });
        let (w, panel) = open_panel(cx);
        start_sleep(cx, &panel);
        unsaved_file(cx, "quit-both", "notes.txt");
        let quits = Rc::new(Cell::new(0));
        try_quit(cx, w, &panel, &quits);
        assert_eq!(
            cx.pending_prompt(),
            Some((
                "Save changes to notes.txt before quitting?".to_string(),
                "Your changes will be lost if you don't save them. A terminal is still running: \
                 sleep. Quitting stops it."
                    .to_string()
            ))
        );
        cx.simulate_prompt_answer("Don't Save");
        cx.run_until_parked();
        assert_eq!(quits.get(), 1);
        assert!(!cx.has_pending_prompt(), "a second question followed");
    }

    /// SITE-1: closing a window asks to save its own unsaved files, not another window's. Cancel
    /// keeps it open; Save writes the file, then closes the window.
    #[gpui_kit::test]
    fn closing_a_window_asks_to_save_its_unsaved_files(cx: &mut TestAppContext) {
        cx.update(crate::editor::testing::init);
        let clean = scratch_dir("close-clean").join("clean.txt");
        std::fs::write(&clean, "clean\n").unwrap();
        let (clean_window, _clean) = open_editor(cx, &clean);
        let (w, _editor, path) = unsaved_file(cx, "close-unsaved", "notes.txt");
        for window in [clean_window, w] {
            cx.update_window(window, |_, window, cx| {
                ask_before_closing(window, WeakEntity::new_invalid(), cx)
            })
            .expect("the test window is open");
        }

        let mut vcx = VisualTestContext::from_window(clean_window, cx);
        assert!(
            vcx.simulate_close(),
            "another window's unsaved file held this one"
        );
        assert!(!cx.has_pending_prompt());

        let mut vcx = VisualTestContext::from_window(w, cx);
        assert!(!vcx.simulate_close(), "the window closed without asking");
        assert_eq!(
            cx.pending_prompt(),
            Some((
                "Save changes to notes.txt before closing this window?".to_string(),
                "Your changes will be lost if you don't save them.".to_string()
            ))
        );
        cx.simulate_prompt_answer("Cancel");
        cx.run_until_parked();
        assert!(cx.windows().contains(&w), "Cancel closed the window");

        let mut vcx = VisualTestContext::from_window(w, cx);
        assert!(!vcx.simulate_close());
        cx.simulate_prompt_answer("Save");
        wait_until(cx, "the window closes", |cx| !cx.windows().contains(&w));
        assert_eq!(on_disk(&path), "typed on disk\n");
    }

    /// Final seams m4 (catalog M5): closing a window while its Dev Tools page installs asks
    /// first, as closing that tab does: the page goes with the window, and the installer with
    /// it. The window closed at once. Another window closes without asking. Quitting asks the
    /// same way, in one question with what the terminals run; Cancel keeps everything, and the
    /// answer goes on.
    #[gpui_kit::test]
    fn closing_or_quitting_asks_while_dev_tools_installs(cx: &mut TestAppContext) {
        cx.update(init);
        let (other, other_panel) = open_panel(cx);
        let (w, panel) = open_panel(cx);
        for (window, panel) in [(other, &other_panel), (w, &panel)] {
            cx.update_window(window, |_, window, cx| {
                ask_before_closing(window, panel.downgrade(), cx)
            })
            .expect("the test window is open");
            let shell = cx.update(|cx| panel.read(cx).tab(0).read(cx).focused_view().clone());
            ready(cx, &shell);
        }
        let page = cx
            .update_window(w, |_, window, cx| {
                cx.new(|cx| ToolsPage::installing_in("bun", window, cx))
            })
            .expect("the test window is open");

        let mut vcx = VisualTestContext::from_window(other, cx);
        assert!(
            vcx.simulate_close(),
            "another window's install held this one"
        );
        assert!(!cx.has_pending_prompt());

        let mut vcx = VisualTestContext::from_window(w, cx);
        assert!(!vcx.simulate_close(), "the window closed without asking");
        assert_eq!(
            cx.pending_prompt(),
            Some((
                "Close this window?".to_string(),
                "Dev Tools is still running: Installing Bun. Closing the window stops it."
                    .to_string()
            ))
        );
        cx.simulate_prompt_answer("Cancel");
        cx.run_until_parked();
        assert!(cx.windows().contains(&w), "Cancel closed the window");

        start_sleep(cx, &panel);
        let quits = Rc::new(Cell::new(0));
        try_quit(cx, w, &panel, &quits);
        assert_eq!(
            cx.pending_prompt(),
            Some((
                "Quit Sawit?".to_string(),
                "A terminal is still running: sleep. Dev Tools is still running: Installing Bun. \
                 Quitting stops them."
                    .to_string()
            ))
        );
        cx.simulate_prompt_answer("Cancel");
        cx.run_until_parked();
        assert_eq!(quits.get(), 0, "Cancel quit");
        try_quit(cx, w, &panel, &quits);
        cx.simulate_prompt_answer("Quit");
        cx.run_until_parked();
        assert_eq!(quits.get(), 1);
        drop(page);
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
