//! Agent Client Protocol agents: the JSON-RPC transport ([`rpc`]), the protocol's client side
//! ([`client`]) and the provider that runs an ACP agent as a child process ([`provider`]).

pub mod client;
mod handler;
#[cfg(test)]
pub mod mock;
pub mod provider;
pub mod rpc;

pub use provider::{ACP, AcpProvider, presets, resolve_program};
