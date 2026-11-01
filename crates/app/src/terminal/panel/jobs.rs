//! The terminals of the Run panel's scripts: starting a script in its shell (or a new one),
//! interrupting it, showing it, and whether it runs.

use std::path::{Path, PathBuf};

use gpui_kit::{App, Context, Window};
use sawit_core::terminal::{Command, PaneId, run_bytes, shell_quote};

use super::{JobStatus, JobTag, Origin, TerminalPanel};

impl TerminalPanel {
    /// Live panes running the script with `key`, oldest first, as the
    /// tab's index and the pane.
    fn job_panes(&self, key: &str, cx: &App) -> Vec<(usize, PaneId)> {
        self.all_panes(cx)
            .filter(|(_, _, v)| {
                let v = v.read(cx);
                !v.exited && v.job.as_ref().is_some_and(|j| j.key == key)
            })
            .map(|(ix, pane, _)| (ix, pane))
            .collect()
    }

    /// The newest live terminal of the script with `key`, if one is open.
    pub fn job_status(&self, key: &str, cx: &App) -> Option<JobStatus> {
        let (ix, pane) = *self.job_panes(key, cx).last()?;
        let view = self.tabs[ix].read(cx).view(pane)?;
        Some(JobStatus {
            running: view.read(cx).foreground_job(),
        })
    }

    /// Where the script with `key` runs, its newest live terminal, as
    /// `app_state` lists it: the tab's index and the pane's place in it.
    pub fn job_terminal(&self, key: &str, cx: &App) -> Option<(usize, usize)> {
        let (ix, pane) = *self.job_panes(key, cx).last()?;
        Some((ix, self.pane_index(ix, pane, cx)?))
    }

    /// Types `command` into the script's terminal: its idle pane when there
    /// is one (a running one is only shown), else a new shell in `dir`. The
    /// old shell may have moved since, and the command is relative to the
    /// script's folder, so it goes back to `dir` first. With `show`, the
    /// terminal shows as that origin allows (one of several started together
    /// passes `None`). Returns the tab and pane, or why no shell could start
    /// (the dock shows it too).
    pub fn start_job(
        &mut self,
        tag: JobTag,
        dir: PathBuf,
        command: &str,
        show: Option<Origin>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(usize, PaneId), String> {
        let (ix, pane, command) = match self.job_panes(&tag.key, cx).last().copied() {
            Some((ix, pane)) => {
                let cwd = self.tabs[ix]
                    .read(cx)
                    .view(pane)
                    .and_then(|v| v.read(cx).cwd());
                (ix, pane, command_in(&dir, command, cwd))
            }
            None => {
                let cmd = Command {
                    cwd: Some(dir),
                    ..Command::default()
                };
                let (ix, pane) = match self.open_tab(cmd, &tag.name, None, cx) {
                    Ok(opened) => opened,
                    Err(e) => {
                        cx.notify();
                        return Err(e);
                    }
                };
                (ix, pane, command.to_string())
            }
        };
        if let Some(view) = self.tabs[ix].read(cx).view(pane).cloned() {
            view.update(cx, |view, _| {
                view.job = Some(tag);
                if view.foreground_job().is_none() {
                    view.touched = true;
                    view.write(run_bytes(&command));
                }
            });
        }
        if let Some(origin) = show {
            self.reveal(ix, pane, origin, window, cx);
        }
        cx.notify();
        Ok((ix, pane))
    }

    /// Interrupts (Ctrl-C) whatever the script's terminals are running.
    pub fn stop_job(&mut self, key: &str, cx: &mut Context<Self>) {
        for (ix, pane) in self.job_panes(key, cx) {
            if let Some(view) = self.tabs[ix].read(cx).view(pane).map(|v| v.read(cx))
                && view.foreground_job().is_some()
            {
                view.write(vec![0x03]);
            }
        }
        cx.notify();
    }

    /// Shows the newest terminal of the script, in whichever pane it is;
    /// false when there is none.
    pub fn show_job(&mut self, key: &str, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let Some((ix, pane)) = self.job_panes(key, cx).last().copied() else {
            return false;
        };
        self.show_pane(ix, pane, window, cx);
        true
    }
}

/// `command` as typed into a shell whose folder is `cwd` (when known) to run
/// in `dir`: as is when the shell is there, else after a `cd` there.
fn command_in(dir: &Path, command: &str, cwd: Option<PathBuf>) -> String {
    let real = dir.canonicalize().unwrap_or_else(|_| dir.to_path_buf());
    if cwd.as_deref() == Some(real.as_path()) {
        command.to_string()
    } else {
        format!("cd -- {} && {command}", shell_quote(dir))
    }
}

#[cfg(test)]
mod tests {
    use super::command_in;

    #[test]
    fn a_script_goes_back_to_its_folder_unless_its_shell_is_there() {
        let dir = std::env::temp_dir().canonicalize().unwrap();
        assert_eq!(
            command_in(&dir, "make build", Some(dir.clone())),
            "make build"
        );
        let moved = command_in(&dir, "make build", Some(dir.join("sub")));
        assert_eq!(
            moved,
            format!("cd -- {} && make build", super::shell_quote(&dir))
        );
        assert_eq!(command_in(&dir, "make build", None), moved);
        let spaced = std::path::Path::new("/tmp/my project");
        assert_eq!(
            command_in(spaced, "pnpm dev", None),
            "cd -- '/tmp/my project' && pnpm dev"
        );
    }
}
