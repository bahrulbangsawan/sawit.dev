//! Unsaved changes kept when Sawit quits without asking. ⌘Q, Restart to Update and closing a
//! window ask to save unsaved files first ([`crate::terminal::end_all`]), but the Dock's Quit,
//! logging out and the install script's `quit app "Sawit"` end the app at once: macOS asks the
//! app nothing it could refuse (GPUI implements no `applicationShouldTerminate:`). So as the app
//! quits, each file editor with unsaved changes keeps them here, in Sawit's own folder, and the
//! next launch opens each file again with its changes back in, unsaved (SITE-1). Changes the user
//! chose Don't Save for aren't kept.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use gpui_kit::{App, Global};
use serde::{Deserialize, Serialize};

/// One file's kept changes: the file, and the text its editor held.
#[derive(Serialize, Deserialize, Debug, PartialEq)]
pub struct Kept {
    pub path: PathBuf,
    pub text: String,
}

/// A folder for kept changes in place of Sawit's own: the tests'.
pub struct KeptFolder(pub PathBuf);

impl Global for KeptFolder {}

/// Where kept changes go: `unsaved/editor/` in Sawit's own folder, beside the copies a canvas that
/// can't save keeps in `unsaved/`.
pub fn folder(cx: &App) -> PathBuf {
    cx.try_global::<KeptFolder>()
        .map_or_else(default_folder, |kept| kept.0.clone())
}

#[cfg(not(test))]
fn default_folder() -> PathBuf {
    sawit_core::instances::AppPaths::default_macos()
        .root
        .join("unsaved")
        .join("editor")
}

/// A folder of the tests' own in place of Sawit's.
#[cfg(test)]
fn default_folder() -> PathBuf {
    std::env::temp_dir().join(format!("sawit-editor-{}-kept", std::process::id()))
}

/// Keeps `path`'s unsaved `text` in a file of its own in `folder`, made new there and never over
/// another: `<file name>.json`, numbered when that is taken. The file it made.
pub fn keep(folder: &Path, path: &Path, text: &str) -> std::io::Result<PathBuf> {
    std::fs::create_dir_all(folder)?;
    let name = path
        .file_name()
        .map_or_else(|| "file".into(), |name| name.to_string_lossy());
    let kept = Kept {
        path: path.to_path_buf(),
        text: text.to_string(),
    };
    let json = serde_json::to_vec(&kept)?;
    sawit_core::nyawit::write_unique(folder, &format!("{name}.json"), &json)
}

/// Every change kept in `folder`, oldest first, each with the file that holds it. Anything else in
/// the folder is left as it is.
pub fn all(folder: &Path) -> Vec<(PathBuf, Kept)> {
    let Ok(entries) = std::fs::read_dir(folder) else {
        return Vec::new();
    };
    let mut found: Vec<(Option<SystemTime>, PathBuf, Kept)> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|file| file.extension().is_some_and(|ext| ext == "json"))
        .filter_map(|file| {
            let kept = serde_json::from_slice(&std::fs::read(&file).ok()?).ok()?;
            let made = std::fs::metadata(&file).and_then(|m| m.modified()).ok();
            Some((made, file, kept))
        })
        .collect();
    found.sort_by(|a, b| (&a.0, &a.1).cmp(&(&b.0, &b.1)));
    found
        .into_iter()
        .map(|(_, file, kept)| (file, kept))
        .collect()
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};
    use std::time::Duration;

    use super::{Kept, all, keep};

    /// Each file's changes are kept in a file of their own, never over another's (two files of
    /// one name), and read back oldest first; what isn't a kept change is left alone.
    #[test]
    fn kept_changes_are_read_back_and_never_written_over() {
        let folder = std::env::temp_dir().join(format!("sawit-kept-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        let first = keep(&folder, Path::new("/p/notes.txt"), "one").unwrap();
        std::thread::sleep(Duration::from_millis(20));
        let second = keep(&folder, Path::new("/q/notes.txt"), "two").unwrap();
        assert_ne!(first, second);
        std::fs::write(folder.join("stray.json"), "not a kept change").unwrap();
        std::fs::write(folder.join("readme.txt"), "{}").unwrap();

        let kept = all(&folder);
        assert_eq!(
            kept,
            vec![
                (
                    first,
                    Kept {
                        path: PathBuf::from("/p/notes.txt"),
                        text: "one".into()
                    }
                ),
                (
                    second,
                    Kept {
                        path: PathBuf::from("/q/notes.txt"),
                        text: "two".into()
                    }
                ),
            ]
        );
        assert!(folder.join("stray.json").exists() && folder.join("readme.txt").exists());
        assert!(all(&folder.join("none")).is_empty());
        let _ = std::fs::remove_dir_all(&folder);
    }
}

#[cfg(test)]
mod quit_tests {
    use std::path::PathBuf;

    use gpui_kit::{
        AppContext as _, Bounds, TestAppContext, WindowBounds, WindowOptions, px, size,
    };

    use super::super::FileEditor;
    use super::super::testing::{buffer, init, open_editor, scratch_dir, type_into, wait_until};
    use super::{Kept, KeptFolder, all, keep};

    /// A folder for the test, with `notes.txt` ("on disk\n") in it, kept changes going to its
    /// `kept/`.
    fn setup(cx: &mut TestAppContext, name: &str) -> (PathBuf, PathBuf) {
        cx.update(init);
        let dir = scratch_dir(name);
        let folder = dir.join("kept");
        cx.update(|cx| cx.set_global(KeptFolder(folder.clone())));
        std::fs::write(dir.join("notes.txt"), "on disk\n").unwrap();
        (dir.join("notes.txt"), folder)
    }

    /// SITE-1: the Dock's Quit, logging out or the install script's quit end the app with no
    /// way to ask. Quitting then keeps each file's unsaved changes, and only those, for the next
    /// launch, and writes nothing over the files themselves. They were lost.
    #[gpui_kit::test]
    fn quitting_keeps_unsaved_changes_for_the_next_launch(cx: &mut TestAppContext) {
        let (path, folder) = setup(cx, "keep");
        let (w, editor) = open_editor(cx, &path);
        type_into(cx, w, &editor, "typed ");
        let clean = path.with_file_name("clean.txt");
        std::fs::write(&clean, "clean\n").unwrap();
        open_editor(cx, &clean);

        // What macOS's terminate: runs once nothing could stop it: every quit hook, then the end.
        cx.quit();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "on disk\n");
        let kept: Vec<Kept> = all(&folder).into_iter().map(|(_, kept)| kept).collect();
        assert_eq!(
            kept,
            vec![Kept {
                path,
                text: "typed on disk\n".into()
            }]
        );
    }

    /// Changes the user chose Don't Save for aren't kept; an edit made after that is.
    #[gpui_kit::test]
    fn changes_the_user_chose_not_to_save_are_not_kept(cx: &mut TestAppContext) {
        let (path, folder) = setup(cx, "discard");
        let other = path.with_file_name("todo.md");
        std::fs::write(&other, "todo\n").unwrap();
        let (w, discarded) = open_editor(cx, &path);
        type_into(cx, w, &discarded, "typed ");
        let (w2, edited) = open_editor(cx, &other);
        type_into(cx, w2, &edited, "a ");
        cx.update(|cx| {
            discarded.update(cx, |e, _| e.discard());
            edited.update(cx, |e, _| e.discard());
        });
        type_into(cx, w2, &edited, "b ");

        cx.quit();
        let kept: Vec<Kept> = all(&folder).into_iter().map(|(_, kept)| kept).collect();
        assert_eq!(
            kept,
            vec![Kept {
                path: other,
                text: "a b todo\n".into()
            }]
        );
    }

    /// The next launch hands each file its kept changes, before or after the file has loaded:
    /// they're back in the buffer, unsaved, the file on disk as it was, and the kept copy goes.
    #[gpui_kit::test]
    fn kept_changes_come_back_unsaved(cx: &mut TestAppContext) {
        let (path, folder) = setup(cx, "restore");
        let from = keep(&folder, &path, "typed on disk\n").unwrap();
        let (_, editor) = cx.update(|cx| {
            let bounds = Bounds::centered(None, size(px(1000.), px(700.)), cx);
            let options = WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..Default::default()
            };
            let (path, from) = (path.clone(), from.clone());
            gpui_kit::open_window(options, cx, move |window, cx| {
                cx.new(|cx| {
                    let mut editor = FileEditor::open(path, None, window, cx);
                    editor.restore("typed on disk\n".into(), from, window, cx);
                    editor
                })
            })
            .expect("the test window opens")
        });
        wait_until(cx, "the file has loaded", |cx| {
            editor.read(cx).state.is_some()
        });
        assert_eq!(buffer(cx, &editor), "typed on disk\n");
        assert!(
            cx.update(|cx| editor.read(cx).dirty),
            "kept changes came back saved"
        );
        assert!(!from.exists(), "the kept copy stayed");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "on disk\n");

        // A file already open and loaded takes them at once.
        let other = path.with_file_name("todo.md");
        std::fs::write(&other, "todo\n").unwrap();
        let from = keep(&folder, &other, "more todo\n").unwrap();
        let (w, open) = open_editor(cx, &other);
        cx.update_window(w, |_, window, cx| {
            open.update(cx, |e, cx| {
                e.restore("more todo\n".into(), from.clone(), window, cx)
            })
        })
        .expect("the test window is open");
        cx.run_until_parked();
        assert_eq!(buffer(cx, &open), "more todo\n");
        assert!(cx.update(|cx| open.read(cx).dirty));
        assert!(!from.exists());
    }
}
