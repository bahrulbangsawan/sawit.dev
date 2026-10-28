//! Prompt manager: a library of saved prompts (sidebar + one tab per prompt).

mod panel;
mod store;
mod view;

pub use panel::{PromptsEvent, PromptsPanel};
pub use store::{PromptStore, PromptStoreEvent};
pub use view::PromptView;
