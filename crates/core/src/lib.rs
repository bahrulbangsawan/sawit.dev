//! Core logic for PG Desk: process management, port probing, connections and
//! queries. Must not depend on GPUI so it can be tested with plain `cargo test`.

pub mod binaries;
mod cmd;
pub mod control;
pub mod db;
pub mod download;
pub mod error;
pub mod files;
pub mod instances;
pub mod owners;
pub mod pgctl;
pub mod ports;
pub mod probe;
pub mod rt;
pub mod terminal;

pub use error::{CoreError, Result};
