//! The browser's copy of mesurer's settings and of the page's origin's workspace, and their writes
//! to Sawit's files (`sawit_core::browser::mesurer::store`): debounced, off the main thread, and
//! flushed on demand (unmount, a new page), when the view goes and when the app quits.
//!
//! The files are the truth between views and launches: a mount reads them again unless this copy
//! has saves the files don't have yet, so a second browser of the same project and origin (another
//! window) mounts with what the first one saved, not with what it held before. Two views saving
//! the same file at once still overwrite each other (the last write wins), as mesurer's own
//! storage does between two tabs.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use gpui_kit::*;
use sawit_core::browser::mesurer::{
    no_links, read_json, remove_file, settings_path, workspace_path, write_json_within,
};
use sawit_core::instances::AppPaths;
use serde_json::{Value, json};

use super::super::BrowserView;

/// How long saves wait for the next one before they are written.
const DEBOUNCE: Duration = Duration::from_millis(300);

/// Where Sawit's data folder is: a folder of the test's own in tests, never the user's.
fn app_paths() -> AppPaths {
    #[cfg(test)]
    {
        AppPaths {
            root: std::env::temp_dir().join(format!("sawit-mesurer-tests-{}", std::process::id())),
        }
    }
    #[cfg(not(test))]
    {
        AppPaths::default_macos()
    }
}

/// One file's pending write: the value, or `None` to remove it.
#[derive(Clone)]
struct Pending {
    path: PathBuf,
    /// The project the file is in, where `.nyawit/` is made; `None` for Sawit's data folder.
    project: Option<PathBuf>,
    value: Option<Value>,
}

#[derive(Default)]
pub(in crate::browser) struct Cache {
    /// The settings, once read or saved.
    settings: Option<Value>,
    /// The origin key whose workspace is held, and the workspace (`None`: there is none).
    workspace: Option<(String, Option<Value>)>,
    /// Writes waiting for the debounce, by file.
    pending: Vec<Pending>,
    /// Writes under way off the main thread, which the quit writes again to be sure of them.
    writing: Vec<Pending>,
    /// Saves queued, and of those the ones written (by the thread that writes them): while they
    /// differ, this copy is newer than the files.
    queued: u64,
    written: Arc<AtomicU64>,
    timer: Option<Task<()>>,
    _quit: Option<Subscription>,
}

impl Cache {
    /// Whether this copy has saves the files don't have yet.
    fn ahead(&self) -> bool {
        self.queued != self.written.load(Ordering::Acquire)
    }

    /// The snapshot `SawitMesurer.mount` takes, `{settings, workspace}`, when this copy holds
    /// both for `origin` and has saves the files don't have yet; `None` when the files must be
    /// read ([`read_snapshot`]), which another view may have written since.
    pub(in crate::browser) fn snapshot(&self, origin: &str) -> Option<Value> {
        if !self.ahead() {
            return None;
        }
        let settings = self.settings.as_ref()?;
        let (held, workspace) = self.workspace.as_ref()?;
        (held == origin).then(|| json!({ "settings": settings, "workspace": workspace }))
    }

    /// What a mount starts from, kept: the files as read, unless saves were queued since, which
    /// are newer.
    pub(in crate::browser) fn loaded(&mut self, origin: &str, snapshot: &Value) {
        let fresh = !self.ahead();
        if fresh || self.settings.is_none() {
            self.settings = Some(snapshot["settings"].clone());
        }
        let held = self.workspace.as_ref().is_some_and(|(o, _)| o == origin);
        if fresh || !held {
            let workspace = Some(snapshot["workspace"].clone()).filter(|w| !w.is_null());
            self.workspace = Some((origin.to_string(), workspace));
        }
    }

    /// Whether this copy holds `origin`'s workspace (which may be none).
    pub(in crate::browser) fn holds(&self, origin: &str) -> bool {
        self.workspace.as_ref().is_some_and(|(o, _)| o == origin)
    }

    /// mesurer's settings as held (`{}` before any).
    pub(in crate::browser) fn settings(&self) -> Value {
        self.settings.clone().unwrap_or_else(|| json!({}))
    }

    /// The workspace of `origin`, if this copy holds it.
    pub(in crate::browser) fn workspace(&self, origin: &str) -> Option<&Value> {
        match &self.workspace {
            Some((held, workspace)) if held == origin => workspace.as_ref(),
            _ => None,
        }
    }

    pub(in crate::browser) fn save_settings(
        &mut self,
        value: Value,
        cx: &mut Context<BrowserView>,
    ) {
        self.settings = Some(value.clone());
        self.queue(settings_path(&app_paths()), None, Some(value), cx);
    }

    pub(in crate::browser) fn clear_settings(&mut self, cx: &mut Context<BrowserView>) {
        self.settings = Some(json!({}));
        self.queue(settings_path(&app_paths()), None, None, cx);
    }

    /// Saves `origin`'s workspace (`None` clears it) for `project`.
    pub(in crate::browser) fn save_workspace(
        &mut self,
        project: Option<&Path>,
        origin: &str,
        value: Option<Value>,
        cx: &mut Context<BrowserView>,
    ) {
        let held = self.workspace.as_ref().is_some_and(|(o, _)| o == origin);
        if held || self.workspace.is_none() {
            self.workspace = Some((origin.to_string(), value.clone()));
        }
        let path = workspace_path(project, &app_paths(), origin);
        self.queue(path, project.map(Path::to_path_buf), value, cx);
    }

    fn queue(
        &mut self,
        path: PathBuf,
        project: Option<PathBuf>,
        value: Option<Value>,
        cx: &mut Context<BrowserView>,
    ) {
        if !self.ahead() {
            // Every write under way has finished.
            self.writing.clear();
        }
        self.pending.retain(|p| p.path != path);
        self.pending.push(Pending {
            path,
            project,
            value,
        });
        self.queued += 1;
        if self._quit.is_none() {
            // Quitting writes what is waiting and under way, before the app goes.
            self._quit = Some(cx.on_app_quit(|this: &mut BrowserView, _| {
                this.mesurer.store.write_now();
                async {}
            }));
        }
        let timer = cx.background_executor().timer(DEBOUNCE);
        self.timer = Some(cx.spawn(async move |this, cx| {
            timer.await;
            let _ = this.update(cx, |this, cx| this.mesurer.store.flush(cx));
        }));
    }

    /// Writes what is waiting now, off the main thread.
    pub(in crate::browser) fn flush(&mut self, cx: &mut Context<BrowserView>) {
        self.timer = None;
        let pending = std::mem::take(&mut self.pending);
        if pending.is_empty() {
            return;
        }
        let queued = self.queued;
        let written = self.written.clone();
        self.writing.extend(pending.iter().cloned());
        let job = crate::rt::spawn_blocking(cx, move || {
            let done = write_all(&pending);
            written.fetch_max(queued, Ordering::AcqRel);
            done
        });
        cx.background_executor()
            .spawn(async move {
                if let Ok(Err(e)) = job.await {
                    eprintln!("sawit: mesurer's files: {e}");
                }
            })
            .detach();
    }

    /// Writes what is waiting and what is under way, here and now (the app quits, the view
    /// goes): a write left to a thread may not finish before the process ends.
    fn write_now(&mut self) {
        let mut all = std::mem::take(&mut self.writing);
        for p in std::mem::take(&mut self.pending) {
            all.retain(|w| w.path != p.path);
            all.push(p);
        }
        self.timer = None;
        if let Err(e) = write_all(&all) {
            eprintln!("sawit: mesurer's files: {e}");
        }
        self.written.fetch_max(self.queued, Ordering::AcqRel);
    }
}

impl Drop for Cache {
    /// The view goes (the tab closed, the app quits): what is waiting is written now.
    fn drop(&mut self) {
        self.write_now();
    }
}

fn write_all(pending: &[Pending]) -> Result<(), String> {
    let mut failed = Vec::new();
    for p in pending {
        if let Err(e) = write_one(p) {
            failed.push(e);
        }
    }
    if failed.is_empty() {
        Ok(())
    } else {
        Err(failed.join("; "))
    }
}

fn write_one(p: &Pending) -> Result<(), String> {
    let root = app_paths().root;
    let base = p.project.as_deref().unwrap_or(&root);
    match &p.value {
        Some(value) => {
            no_links(base, &p.path)?;
            if let (Some(project), Some(dir)) = (&p.project, p.path.parent()) {
                sawit_core::nyawit::ensure(project, dir)
                    .map_err(|e| format!("{} can't be made: {e}", dir.display()))?;
            }
            write_json_within(base, &p.path, value)
        }
        None => remove_file(&p.path),
    }
}

/// Reads mesurer's settings and `origin`'s workspace in `project` from Sawit's files, as
/// `SawitMesurer.mount` takes them. A file that can't be read is left out (and said why).
pub(in crate::browser) fn read_snapshot(project: Option<&Path>, origin: &str) -> Value {
    let paths = app_paths();
    let read = |path: &Path| match read_json(path) {
        Ok(value) => value,
        Err(e) => {
            eprintln!("sawit: mesurer's files: {e}");
            None
        }
    };
    let settings = read(&settings_path(&paths)).filter(Value::is_object);
    let workspace = read(&workspace_path(project, &paths, origin)).filter(Value::is_object);
    json!({ "settings": settings.unwrap_or_else(|| json!({})), "workspace": workspace })
}

/// Where `origin`'s workspace is kept for `project`.
pub(in crate::browser) fn workspace_file(project: Option<&Path>, origin: &str) -> PathBuf {
    workspace_path(project, &app_paths(), origin)
}
