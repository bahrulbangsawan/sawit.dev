//! Whether a server can bind a port: who already listens on it, and on which addresses.

use super::{BindAddr, Listener};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PortVerdict {
    Free,
    /// Someone holds the exact address we want: starting will fail.
    Conflict {
        same_addr: Vec<Listener>,
        shadowing: Vec<Listener>,
    },
    /// Overlapping binds of different specificity: starting may succeed but
    /// clients reach different servers depending on the address they use.
    Shadowing {
        listeners: Vec<Listener>,
    },
}

/// Judges whether binding `intended` addresses on `port` collides with `listeners`.
pub fn analyze(port: u16, intended: &[BindAddr], listeners: &[Listener]) -> PortVerdict {
    let mut same_addr = Vec::new();
    let mut shadowing = Vec::new();
    for l in listeners.iter().filter(|l| l.port == port) {
        let relevant: Vec<_> = intended.iter().filter(|a| a.overlaps(&l.addr)).collect();
        if relevant.iter().any(|a| **a == l.addr) {
            same_addr.push(l.clone());
        } else if relevant
            .iter()
            .any(|a| a.is_wildcard() || l.addr.is_wildcard())
        {
            shadowing.push(l.clone());
        }
    }
    match (same_addr.is_empty(), shadowing.is_empty()) {
        (true, true) => PortVerdict::Free,
        (true, false) => PortVerdict::Shadowing {
            listeners: shadowing,
        },
        (false, _) => PortVerdict::Conflict {
            same_addr,
            shadowing,
        },
    }
}
