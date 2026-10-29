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
