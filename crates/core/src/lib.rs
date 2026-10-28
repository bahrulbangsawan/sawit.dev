//! Core logic for PG Desk: process management, port probing, connections and
//! queries. Must not depend on GPUI so it can be tested with plain `cargo test`.

pub mod error;
pub mod owners;
pub mod pgctl;
pub mod ports;
pub mod rt;

pub use error::{CoreError, Result};
