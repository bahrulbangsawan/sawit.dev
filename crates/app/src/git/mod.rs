//! Git and GitHub: the sidebar (changes, commit, branches, sync), the
//! commit graph, file diffs, and the GitHub tab (sign-in, pull requests,
//! issues, comments, your repositories).

mod commit;
mod diff;
mod github;
mod graph;
mod panel;
mod store;
mod thread;

pub use commit::CommitView;
pub use diff::{DiffView, DiffViewEvent};
pub use github::GitHubView;
pub use graph::{GraphEvent, GraphView};
pub use panel::{GitPanel, GitPanelEvent};
pub use store::GitStore;

use gpui_kit::*;

actions!(
    git,
    [
        /// Commits the staged changes with the message being written.
        Commit,
        /// Sends the comment being written on a pull request or an issue.
        SendComment
    ]
);

/// Binds ⌘↩ in the commit message and the comment boxes, over the boxes' own ⌘↩ (`input::Enter`),
/// as the SQL editor's runs its query: "X > Input" matches as deep as the box's binding and is
/// bound later, so it wins.
pub fn init(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("cmd-enter", Commit, Some("GitCommitMessage > Input")),
        KeyBinding::new("cmd-enter", SendComment, Some("GitComment > Input")),
    ]);
}

#[cfg(test)]
mod tests {
    use gpui_kit::TestAppContext;

    /// The commit and comment boxes say which keys send them, as the keymap binds them: ⌘↩ in
    /// the box (`GitCommitMessage > Input`), which takes the box's own ⌘↩.
    #[gpui_kit::test]
    fn the_boxes_say_their_keys(cx: &mut TestAppContext) {
        cx.update(|cx| {
            gpui_kit::init(cx);
            super::init(cx);
        });
        cx.update(|cx| {
            assert_eq!(
                super::panel::commit_placeholder(cx),
                "Commit message (⌘↩ to commit)"
            );
            assert_eq!(
                super::thread::comment_placeholder(cx),
                "Leave a comment (Markdown, ⌘↩ to send)"
            );
        });
    }
}
