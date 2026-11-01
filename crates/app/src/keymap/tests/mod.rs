//! The keymap store's tests, over defaults of their own and a keymap file in a scratch folder:
//! the helpers they share here, the tests by what they cover in the modules below.

use std::path::PathBuf;

use gpui_kit::{App, Entity, KeyBinding, KeyContext, Keystroke, OperatingSystem, TestAppContext};

use super::{KeymapStore, Source, init_at, store};

mod binding;
mod changes;
mod on_disk;

gpui_kit::actions!(keymap_test, [Close, CloseAll, Other]);

/// A keymap file of the test's own, removed when the test ends.
pub(super) struct Scratch(pub(super) PathBuf);

impl Scratch {
    pub(super) fn new(test: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("sawit-keymap-{}-{test}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        Self(dir)
    }

    pub(super) fn file(&self) -> PathBuf {
        self.0.join("keymap.json")
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// The app's defaults for the tests: ⌘W closes, ⌘K W closes all, in `Workspace`.
fn start(cx: &mut TestAppContext, scratch: &Scratch) -> Entity<KeymapStore> {
    cx.update(|cx| {
        cx.bind_keys([
            KeyBinding::new("cmd-w", Close, Some("Workspace")),
            KeyBinding::new("cmd-k w", CloseAll, Some("Workspace")),
        ]);
        init_at(scratch.file(), cx);
        store(cx)
    })
}

/// What `keystroke` runs in the workspace, by action name.
pub(super) fn runs(cx: &App, keystroke: &str) -> Vec<&'static str> {
    let typed: Vec<Keystroke> = keystroke
        .split(' ')
        .map(|k| Keystroke::parse_with_os(k, OperatingSystem::Mac).unwrap())
        .collect();
    let contexts = [KeyContext::parse("Workspace").unwrap()];
    let keymap = cx.key_bindings();
    let keymap = keymap.borrow();
    let (bindings, _) = keymap.bindings_for_input(&typed, &contexts);
    bindings.iter().map(|b| b.action().name()).collect()
}

fn default_row(store: &Entity<KeymapStore>, cx: &App, action: &str) -> Source {
    store
        .read(cx)
        .rows(false, cx)
        .into_iter()
        .find(|row| row.action.as_ref() == action)
        .map(|row| row.source)
        .unwrap()
}
