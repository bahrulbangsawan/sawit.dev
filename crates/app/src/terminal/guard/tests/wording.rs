//! What the question says: what would end, and which files would lose their changes.

use super::super::{Ending, detail, save_detail, save_question};

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
