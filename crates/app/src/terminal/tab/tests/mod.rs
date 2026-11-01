//! The tab's GPUI tests, with real shells, by what they cover.

mod exit;
mod focus;
mod header;
mod keys;
mod split;

/// A stand-in SSH server "test": a session that ends once it reads a
/// line.
fn test_remote() -> super::super::Remote {
    super::super::Remote {
        key: "test".into(),
        name: "test".into(),
        launch: sawit_core::ssh::Launch {
            program: "/bin/sh".into(),
            args: vec!["-c".into(), "read line".into()],
            env: Vec::new(),
        },
    }
}
