//! Core logic for Sawit: process management, port probing, connections and
//! queries. Must not depend on GPUI so it can be tested with plain `cargo test`.

pub mod binaries;
pub mod cli;
mod cmd;
pub mod control;
pub mod db;
pub mod download;
pub mod error;
pub mod files;
pub mod format;
pub mod instances;
pub mod mcp_clients;
pub mod media;
pub mod owners;
pub mod pdf;
pub mod pgctl;
pub mod ports;
pub mod probe;
pub mod prompts;
pub mod rt;
pub mod terminal;
pub mod update;

pub use error::{CoreError, Result};
