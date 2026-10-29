//! Core logic for Sawit: process management, port probing, connections and
//! queries. Must not depend on GPUI so it can be tested with plain `cargo test`.

pub mod app_control;
pub mod binaries;
pub mod cli;
mod cmd;
pub mod control;
pub mod db;
pub mod dev_tools;
pub mod download;
pub mod error;
pub mod files;
pub mod format;
pub mod git;
pub mod github;
pub mod instances;
pub mod kebun;
pub mod keychain;
pub mod mcp_auth;
pub mod mcp_catalog;
pub mod mcp_clients;
pub mod media;
pub mod mermaid;
pub mod network;
pub mod owners;
pub mod pdf;
pub mod pgctl;
pub mod ports;
pub mod probe;
pub mod prompts;
pub mod recent;
pub mod rt;
pub mod scripts;
pub mod search;
pub mod shell_config;
pub mod ssh;
pub mod terminal;
pub mod update;

pub use error::{CoreError, Result};
