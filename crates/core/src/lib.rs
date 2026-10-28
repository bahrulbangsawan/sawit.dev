//! Core logic for PG Desk: process management, port probing, connections and
//! queries. Must not depend on GPUI so it can be tested with plain `cargo test`.

mod cmd;
pub mod error;
pub mod owners;
pub mod pgctl;
pub mod ports;
pub mod probe;
pub mod rt;

pub use error::{CoreError, Result};
