//! Git and GitHub: the sidebar (changes, commit, branches, sync), the
//! commit graph, file diffs, the app's one GitHub sign-in, and the GitHub tab
//! (pull requests, issues, comments, your repositories).

pub mod account;
mod commit;
mod diff;
mod github;
mod graph;
mod panel;
mod store;
mod thread;

pub use account::{GitHubAccount, OpenGitHub, SignInToGitHub};
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
    use gpui_kit::{AppContext as _, TestAppContext};

    /// Clone from GitHub in the Git panel (a window with no folder) is off while signed out,
    /// saying why, and turns on as the one sign-in happens, without reopening anything.
    #[gpui_kit::test]
    fn clone_from_github_waits_for_the_sign_in(cx: &mut TestAppContext) {
        cx.update(|cx| {
            gpui_kit::init(cx);
            super::init(cx);
        });
        let (_, panel) = cx.update(|cx| {
            gpui_kit::open_window(Default::default(), cx, |window, cx| {
                // No folder open: the panel offers Clone from GitHub.
                let store = cx.new(super::GitStore::new);
                cx.new(|cx| super::GitPanel::new(store, window, cx))
            })
            .unwrap()
        });
        cx.run_until_parked();
        cx.update(|cx| {
            assert_eq!(
                panel.read(cx).clone_blocked(cx).as_deref(),
                Some("Sign in to GitHub to clone your repositories")
            )
        });
        let account = cx.update(super::account::global);
        let me = sawit_core::github::Account {
            login: "me".into(),
            ..Default::default()
        };
        account.update(cx, |a, cx| a.signed_in("gho_1".into(), me, None, cx));
        cx.run_until_parked();
        cx.update(|cx| assert_eq!(panel.read(cx).clone_blocked(cx), None));
        account.update(cx, |a, cx| a.sign_out(cx));
        cx.run_until_parked();
        cx.update(|cx| assert!(panel.read(cx).clone_blocked(cx).is_some()));
    }

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
