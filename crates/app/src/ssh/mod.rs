//! SSH to remote servers: a sidebar of saved servers and `~/.ssh/config`
//! hosts, a tab to edit each one, and sessions in the terminal dock.

mod panel;
mod store;
mod view;

pub use panel::{SshEvent, SshPanel};
pub use store::{SshStore, SshStoreEvent, alias_key, saved_key};
pub use view::{SshHostEvent, SshHostView};
