//! Lane 2 (mesurer-agents): mesurer's comment threads and page markup as Markdown, for Send, Copy
//! and `browser_annotations`. They live with the notes they go out with, in
//! `crate::annotations`: comment threads as rows (`annotations::Source::Comment`,
//! `annotations::mesurer::threads_of`), the rest of a page's markup as its own section
//! (`annotations::PageMarkup`), and Sawit's changes to a workspace (`annotations::mesurer::edit`).
