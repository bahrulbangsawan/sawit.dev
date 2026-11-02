//! The screens folder, watched: a change to a screen's files on disk, by an agent, the user's
//! editor or anything else, reloads the frames that show it, which settle and are snapshotted
//! again, so the canvas follows the files on its own. Changes that come close together reload a
//! screen once ([`GATHER`]), and Sawit's own writes, which reload at once, aren't taken twice
//! ([`QUIET`]).
//!
//! The folder is watched once the page shows a screen and the folder is there; until then it is
//! looked for again now and then ([`RETRY`]).

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use futures::channel::mpsc::UnboundedSender;
use gpui_kit::*;
use notify::{EventKind, RecursiveMode, Watcher as _};

use super::{BoardView, Source, WebFrames, WebMsg, WebNode};

/// How long changes are gathered before the screens they touch reload.
const GATHER: Duration = Duration::from_millis(60);
/// How long after Sawit writes a screen itself the watcher's news of that write is let be.
const QUIET: Duration = Duration::from_millis(400);
/// How often a screens folder that isn't there yet, or couldn't be watched, is tried again.
const RETRY: Duration = Duration::from_secs(2);

/// What watches a canvas's screens folder.
pub(in crate::board) struct Watch {
    _watcher: notify::RecommendedWatcher,
}

impl Watch {
    /// Watches `dir`, sending the names of the screens whose files change to `to_view`.
    fn new(dir: &Path, to_view: UnboundedSender<WebMsg>) -> notify::Result<Watch> {
        let root = dir.to_path_buf();
        let mut watcher =
            notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
                let Ok(event) = event else {
                    return;
                };
                if matches!(event.kind, EventKind::Access(_)) {
                    return;
                }
                let names = screens_of(&root, &event.paths);
                if !names.is_empty() {
                    let _ = to_view.unbounded_send(WebMsg::Changed(names));
                }
            })?;
        watcher.watch(dir, RecursiveMode::Recursive)?;
        Ok(Watch { _watcher: watcher })
    }
}

/// The screens `paths` are in, in the screens folder `root`: the first part of each path under it.
pub(in crate::board) fn screens_of(root: &Path, paths: &[PathBuf]) -> Vec<String> {
    let root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    let mut names: Vec<String> = paths
        .iter()
        .filter_map(|path| {
            let path = path.strip_prefix(&root).ok()?;
            let first = path.components().next()?;
            let name = first.as_os_str().to_str()?;
            sawit_board::screens::is_name(name).then(|| name.to_string())
        })
        .collect();
    names.sort();
    names.dedup();
    names
}

impl WebFrames {
    /// Watches the screens folder once the page shows a screen and the folder is there.
    pub(super) fn keep_watching(&mut self, nodes: &[WebNode]) {
        if self.watch.is_some() {
            return;
        }
        let shows_screens = nodes
            .iter()
            .any(|node| matches!(node.spec.source, Source::Screen(_)));
        let now = Instant::now();
        let due = self
            .watch_tried
            .is_none_or(|at| now.duration_since(at) >= RETRY);
        if !shows_screens || !due {
            return;
        }
        self.watch_tried = Some(now);
        if !self.dir.is_dir() {
            return;
        }
        match Watch::new(&self.dir, self.to_view.clone()) {
            Ok(watch) => self.watch = Some(watch),
            Err(error) => log::warn!("couldn't watch {}: {error}", self.dir.display()),
        }
    }

    /// Sawit wrote screen `name`'s files itself: the watcher's news of it is let be a moment.
    pub(in crate::board) fn wrote(&mut self, name: &str) {
        self.quiet.insert(name.to_string(), Instant::now() + QUIET);
    }
}

impl BoardView {
    /// The files of screens `names` changed on disk: they reload together once the changes that
    /// come with these are in, unless Sawit wrote them itself just now.
    pub(in crate::board) fn web_files_changed(
        &mut self,
        names: Vec<String>,
        cx: &mut Context<Self>,
    ) {
        let Some(web) = self.web.as_mut() else {
            return;
        };
        let now = Instant::now();
        web.quiet.retain(|_, until| *until > now);
        let changed: Vec<String> = (names.into_iter())
            .filter(|name| !web.quiet.contains_key(name))
            .collect();
        if changed.is_empty() {
            return;
        }
        web.dirty.extend(changed);
        if std::mem::replace(&mut web.flushing, true) {
            return;
        }
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(GATHER).await;
            let _ = this.update(cx, |this, cx| this.web_reload_dirty(cx));
        })
        .detach();
    }

    /// Reloads the frames of the screens whose files changed.
    fn web_reload_dirty(&mut self, cx: &mut Context<Self>) {
        let Some(web) = self.web.as_mut() else {
            return;
        };
        web.flushing = false;
        let dirty: Vec<String> = web.dirty.drain().collect();
        for name in dirty {
            self.web_reload_screen(&name, cx);
        }
    }

    /// Reloads the frames that show screen `name` now ([`WebFrames::reload`]).
    pub(in crate::board) fn web_reload_screen(&mut self, name: &str, cx: &mut Context<Self>) {
        let Some(web) = self.web.as_mut() else {
            return;
        };
        for id in web.showing(name) {
            web.reload(id);
        }
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use super::{Watch, WebMsg};

    /// A file written in a screen's folder, as an agent or an editor writes it, reaches the canvas
    /// as a change of that screen.
    #[test]
    fn a_change_on_disk_names_its_screen() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("sign-in/css")).unwrap();
        let (tx, mut rx) = futures::channel::mpsc::unbounded();
        let _watch = Watch::new(dir.path(), tx).unwrap();
        // FSEvents starts its stream a moment after it is made.
        std::thread::sleep(Duration::from_millis(300));
        std::fs::write(dir.path().join("sign-in/css/site.css"), "h1{}").unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            match rx.try_recv() {
                Ok(WebMsg::Changed(names)) => {
                    assert_eq!(names, ["sign-in"]);
                    break;
                }
                _ if Instant::now() > deadline => panic!("no change came within 5 s"),
                _ => std::thread::sleep(Duration::from_millis(50)),
            }
        }
    }
}
