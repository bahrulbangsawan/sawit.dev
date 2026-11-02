//! The `sawit` command in a terminal: whether it reaches this app (the status bar offers "Install
//! CLI" while it doesn't), and installing it.

use std::path::{Path, PathBuf};

use gpui_kit::*;
use sawit_core::cli::{CLI_LINK, LinkState, install_link, install_link_as_admin, link_state};
use sawit_core::probe::find_in_path;

use super::frontmost;
use crate::rt;

/// Whether `sawit` in the terminal reaches this app; the title bar's
/// "Install CLI" button shows only while it doesn't.
pub struct CliStatus {
    pub installed: bool,
}

impl CliStatus {
    pub(super) fn detect() -> bool {
        let Ok(exe) = std::env::current_exe().and_then(|e| e.canonicalize()) else {
            return false;
        };
        link_state(Path::new(CLI_LINK), &exe) == LinkState::Installed
            || find_in_path("sawit").and_then(|p| p.canonicalize().ok()) == Some(exe)
    }
}

pub(super) struct GlobalCliStatus(pub(super) Entity<CliStatus>);

impl Global for GlobalCliStatus {}

pub fn cli_status(cx: &App) -> Entity<CliStatus> {
    cx.global::<GlobalCliStatus>().0.clone()
}

/// Links `/usr/local/bin/sawit` to this app (menu and status bar), asking
/// for the admin password when the folder is root-owned.
pub fn install_cli(cx: &mut App) {
    let Ok(exe) = std::env::current_exe().and_then(|e| e.canonicalize()) else {
        return;
    };
    let link = PathBuf::from(CLI_LINK);
    let rx = rt::spawn(cx, async move {
        match install_link(&link, &exe) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
                install_link_as_admin(&link, &exe)
                    .await
                    .map_err(|e| e.to_string())
            }
            Err(e) => Err(e.to_string()),
        }
    });
    cx.spawn(async move |cx| {
        let Ok(result) = rx.await else { return };
        cx.update(|cx| {
            cli_status(cx).update(cx, |s, cx| {
                s.installed = CliStatus::detect();
                cx.notify();
            });
            let (level, title, detail) = match result {
                Ok(()) => {
                    let in_path = std::env::var_os("PATH").is_some_and(|p| {
                        std::env::split_paths(&p).any(|d| d == Path::new("/usr/local/bin"))
                    });
                    let mut detail = format!(
                        "{CLI_LINK} is installed. From a terminal:\n  sawit .             open this folder\n  sawit file.rs:42    open a file at line 42\n  sawit --help        all options"
                    );
                    if !in_path {
                        detail.push_str("\n\n/usr/local/bin isn't in your shell's PATH. Add this line to ~/.zshrc:\n  export PATH=\"/usr/local/bin:$PATH\"");
                    }
                    (PromptLevel::Info, "‘sawit’ command installed", detail)
                }
                Err(e) => (PromptLevel::Critical, "Unable to install the ‘sawit’ command", e),
            };
            if let Some((handle, _)) = frontmost(cx) {
                let _ = handle.update(cx, |_, window, cx| {
                    window.prompt(level, title, Some(&detail), &["OK"], cx)
                });
            }
        });
    })
    .detach();
}
