//! The project tests: the keyboard going to a file shown from the Files tab, and viewer and search
//! tabs following a move.

use std::time::{Duration, Instant};

use gpui_kit::{
    AnyWindowHandle, AppContext as _, Context, Entity, FocusHandle, Focusable as _, IntoElement,
    ParentElement as _, Render, Styled as _, TestAppContext, Window, div,
    prelude::InteractiveElement as _,
};

use super::{FileEditor, FolderSearch, ImageView, PaneItem, give_keyboard};
use crate::files::Scratch;
use crate::workspace::file_tabs::follow_tab;

/// A window with the Files tree's keyboard, the workspace's, and a text file's and an
/// image's tab.
struct Window2 {
    tree: FocusHandle,
    home: FocusHandle,
    editor: Entity<FileEditor>,
    image: Entity<ImageView>,
}

impl Render for Window2 {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .track_focus(&self.home)
            .child(div().track_focus(&self.tree))
            .child(self.editor.clone())
            .child(self.image.clone())
    }
}

/// Whether `handle`, picked from the window's view, has the keyboard.
fn has(
    window: AnyWindowHandle,
    view: &Entity<Window2>,
    cx: &mut TestAppContext,
    handle: impl Fn(&Window2, &gpui_kit::App) -> FocusHandle,
) -> bool {
    cx.update_window(window, |_, window, cx| {
        handle(view.read(cx), cx).is_focused(window)
    })
    .unwrap()
}

/// Gives the keyboard to the tree, then hands it on as the Files tab's OpenFile does.
fn open_from_tree(
    window: AnyWindowHandle,
    view: &Entity<Window2>,
    cx: &mut TestAppContext,
    item: impl Fn(&Window2) -> PaneItem,
) {
    cx.update_window(window, |_, window, cx| {
        let (tree, home, item) = {
            let v = view.read(cx);
            (v.tree.clone(), v.home.clone(), item(v))
        };
        window.focus(&tree, cx);
        give_keyboard(Some(&item), &home, window, cx);
    })
    .unwrap();
    cx.run_until_parked();
}

/// The file the Files tab opens or shows takes the keyboard from the tree (FILES-1): a text
/// file's editor, its text once read; an image, which takes no keys, leaves them with the
/// workspace. Reopening a file whose tab was open left them in the tree, unseen, where ⌫,
/// ⌘Z, ⌘D and ⌘V meant for the file acted on files.
#[gpui_kit::test]
fn a_file_shown_from_the_files_tab_takes_the_keyboard(cx: &mut TestAppContext) {
    // The files are read on the runtime's real threads.
    cx.executor().allow_parking();
    cx.update(|cx| {
        gpui_kit::init(cx);
        cx.set_global(crate::rt::Rt(sawit_core::rt::Runtime::start()));
    });
    let dir = Scratch::new("give-keyboard");
    let (notes, picture) = (dir.0.join("notes.md"), dir.0.join("picture.png"));
    std::fs::write(&notes, "notes\n").unwrap();
    std::fs::write(&picture, b"not a picture").unwrap();
    let (window, view) = cx.update(|cx| {
        gpui_kit::open_window(Default::default(), cx, |window, cx| {
            let editor = cx.new(|cx| FileEditor::open(notes, None, window, cx));
            let image = cx.new(|cx| ImageView::new(picture, cx));
            cx.new(|cx| Window2 {
                tree: cx.focus_handle(),
                home: cx.focus_handle(),
                editor,
                image,
            })
        })
        .unwrap()
    });
    let editor = |v: &Window2| PaneItem::File(v.editor.clone());
    let text = |v: &Window2, cx: &gpui_kit::App| {
        let editor = v.editor.read(cx);
        editor
            .text_focus(cx)
            .unwrap_or_else(|| v.editor.focus_handle(cx))
    };

    // Before the file is read, the editor has the keyboard; its text takes it once read.
    open_from_tree(window, &view, cx, editor);
    assert!(has(window, &view, cx, text), "the tree kept the keyboard");
    let deadline = Instant::now() + Duration::from_secs(20);
    while cx.update(|cx| view.read(cx).editor.read(cx).text_focus(cx).is_none()) {
        assert!(Instant::now() < deadline, "the file was never read");
        std::thread::sleep(Duration::from_millis(10));
        cx.run_until_parked();
    }
    // Shown again from the tree, as a file whose tab is open is.
    open_from_tree(window, &view, cx, editor);
    assert!(
        has(window, &view, cx, text),
        "the text doesn't have the keyboard"
    );

    let image = |v: &Window2| PaneItem::Other {
        view: v.image.clone().into(),
        key: "file\u{0}picture.png".into(),
        title: "picture.png".into(),
        icon: gpui_kit::assets::IconName::Image,
        file: None,
    };
    open_from_tree(window, &view, cx, image);
    assert!(has(window, &view, cx, |v, _| v.home.clone()));
    assert!(!has(window, &view, cx, |v, _| v.tree.clone()));
}

/// The tabs of a renamed or moved file follow it (FILES-14): an image's, a PDF's or a
/// video's, as a text file's and a canvas file's did, and a Find in Folder tab its folder
/// and the files it found. They kept the old path: the file renamed opened in a second tab,
/// `sawit --wait` waited on a path that was gone, and a search ran where its folder was.
#[gpui_kit::test]
fn viewer_and_search_tabs_follow_a_move(cx: &mut TestAppContext) {
    // The image is looked at on the runtime's real threads.
    cx.executor().allow_parking();
    cx.update(|cx| {
        gpui_kit::init(cx);
        cx.set_global(crate::rt::Rt(sawit_core::rt::Runtime::start()));
    });
    let dir = Scratch::new("tabs-follow-a-move");
    let (a, b) = (dir.0.join("a.png"), dir.0.join("b.png"));
    let (docs, notes) = (dir.0.join("docs"), dir.0.join("notes"));
    std::fs::write(&a, b"not a picture").unwrap();
    std::fs::create_dir(&docs).unwrap();
    let (image, search) = (a.clone(), docs.clone());
    let (image, search) = cx.update(|cx| {
        let (_, pair) = gpui_kit::open_window(Default::default(), cx, |window, cx| {
            let image = cx.new(|cx| ImageView::new(image, cx));
            let search = cx.new(|cx| FolderSearch::new(search, window, cx));
            cx.new(|_| Pair(image, search))
        })
        .unwrap();
        let pair = pair.read(cx);
        (pair.0.clone(), pair.1.clone())
    });
    let mut image_tab = PaneItem::Other {
        view: image.clone().into(),
        key: format!("file\u{0}{}", a.display()).into(),
        title: "a.png".into(),
        icon: gpui_kit::assets::IconName::Image,
        file: Some(a.clone()),
    };
    let mut search_tab = PaneItem::Other {
        view: search.clone().into(),
        key: FolderSearch::key(&docs).into(),
        title: "Search: docs".into(),
        icon: gpui_kit::assets::IconName::Search,
        file: None,
    };

    cx.update(|cx| {
        follow_tab(&mut image_tab, &a, &b, cx);
        follow_tab(&mut search_tab, &docs, &notes, cx);
        let PaneItem::Other {
            key, title, file, ..
        } = &image_tab
        else {
            unreachable!()
        };
        assert_eq!(key.as_ref(), format!("file\u{0}{}", b.display()));
        assert_eq!((title.as_ref(), file.as_ref()), ("b.png", Some(&b)));
        assert_eq!(image.read(cx).path, b);
        let PaneItem::Other { key, title, .. } = &search_tab else {
            unreachable!()
        };
        assert_eq!(key.as_ref(), FolderSearch::key(&notes));
        assert_eq!(title.as_ref(), "Search: notes");
        assert_eq!(search.read(cx).dir, notes);
    });
}

/// A window's view holding an image's and a search's tab.
struct Pair(Entity<ImageView>, Entity<FolderSearch>);

impl Render for Pair {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .child(self.0.clone())
            .child(self.1.clone())
    }
}
