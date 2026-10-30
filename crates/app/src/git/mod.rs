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
