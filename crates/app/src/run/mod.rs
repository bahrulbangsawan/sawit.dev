//! Run sidebar: the project's scripts and the user's saved commands, each
//! started in its own terminal with one click.

mod panel;
mod store;

pub use panel::{RunEvent, RunPanel};
pub use store::RunStore;
