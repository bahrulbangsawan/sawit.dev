//! Herdr (herdr.dev): the desktop app, whose agents run in herdr-core, and the original
//! terminal app. They install, update and live in different places, so they are two tools.
//! What follows was read from their install scripts and repositories on 2026-11-06 and checked
//! against installs on a Mac with Apple silicon.

use super::super::{Category, Latest, Tool};
use super::tool;

/// The Herdr app (github.com/herdrdev/herdr-app, Apache-2.0): a native window for coding
/// agents, which run in herdr-core (github.com/herdrdev/herdr-core, Apache-2.0), a server that
/// keeps them going while the window is closed.
///
/// Its installer, releases.herdr.dev/app/install.sh, fails on an Intel Mac (macOS builds are
/// Apple silicon only; under Rosetta it still installs the arm64 build). It puts `Herdr.app`
/// in ~/Applications (not /Applications), links ~/.local/bin/herdr-app into it, and installs
/// herdr-core only when there's none (releases.herdr.dev/core/install.sh): `Herdr Core.app`
/// under ~/.local/share/herdr-core, linked as ~/.local/bin/herdr-core and `hc`. The version is
/// the bundle's `CFBundleShortVersionString` (`herdr-app --version` says the same, "0.1.13");
/// the latest is in releases.herdr.dev/app/latest.json, which the app's own updater checks
/// every hour, and which running the installer again installs. A `Herdr.app` dragged into
/// /Applications from `Herdr.dmg` has no `herdr-app` link into it, so Sawit leaves it alone.
///
/// Uninstalling closes the window, stops herdr-core (`hc server stop`: the agents in it stop)
/// and removes what the installers put in place. The settings, ~/.config/herdr-app
/// (`settings.json`, `keys.json`, `layouts/`, `mods/`, `themes/`, `AGENTS.md`), and
/// herdr-core's ~/.config/herdr-core and ~/.local/state/herdr-core stay: deleting them is a
/// choice of its own.
pub(super) const APP: Tool = Tool {
    owns: &["~/.local/bin/herdr-app"],
    also: &["~/Applications/Herdr.app", "/Applications/Herdr.app"],
    latest: Latest::Manifest("https://releases.herdr.dev/app/latest.json"),
    apple_silicon: true,
    uninstall: Some(
        r#"osascript -e 'if application id "dev.herdr.app" is running then tell application id "dev.herdr.app" to quit' >/dev/null 2>&1 || true
hc="$HOME/.local/bin/herdr-core"
if [ -x "$hc" ]; then "$hc" server stop >/dev/null 2>&1 || true; fi
rm -f "$HOME/.local/bin/herdr-app"
rm -rf "$HOME/Applications/Herdr.app"
for name in herdr-core hc; do
  link="$HOME/.local/bin/$name"
  case "$(readlink "$link" 2>/dev/null)" in
    "$HOME/.local/share/herdr-core/"* | herdr-core) rm -f "$link" ;;
    "") if [ "$name" = herdr-core ] && [ -f "$link" ]; then rm -f "$link"; fi ;;
  esac
done
rm -rf "$HOME/.local/share/herdr-core""#,
    ),
    keeps: &[
        "~/.config/herdr-app",
        "~/.config/herdr-core",
        "~/.local/state/herdr-core",
    ],
    warning: Some("Agents running in herdr-core stop, and the Herdr window closes."),
    ..tool(
        "herdr-app",
        "Herdr",
        "Desktop app for coding agents, which keep running in herdr-core when the window closes.",
        Category::Agent,
        "",
        "curl -fsSL https://releases.herdr.dev/app/install.sh | sh",
    )
};

/// The Herdr TUI (github.com/herdrdev/herdr, Apache-2.0): the original Herdr, one binary with
/// its own server; its agents don't show in the app.
///
/// Its installer, herdr.dev/install.sh, downloads the binary herdr.dev/latest.json names for
/// this Mac (Intel or Apple silicon), checks its SHA-256 and puts it in ~/.local/bin/herdr
/// (`HERDR_INSTALL_DIR` picks another folder); nothing else. `herdr --version` prints "herdr
/// 0.9.5", and `herdr update` updates a copy installed so ("already up to date (0.9.5)").
/// Homebrew's core has a `herdr` formula (no tap), and mise and Nix have it too: those copies
/// are found, and left to their own manager. Uninstalling stops its server (`herdr server
/// stop`, its own, not herdr-core) and removes the binary; its settings in ~/.config/herdr and
/// state in ~/.local/state/herdr stay.
pub(super) const TUI: Tool = Tool {
    owns: &["~/.local/bin/herdr"],
    also: &["~/.local/bin/herdr"],
    latest: Latest::GitHub("herdrdev/herdr"),
    upgrade: Some("herdr update"),
    uninstall: Some(
        r#""$HOME/.local/bin/herdr" server stop >/dev/null 2>&1 || true
rm -f "$HOME/.local/bin/herdr""#,
    ),
    keeps: &["~/.config/herdr", "~/.local/state/herdr"],
    warning: Some("Agents running in Herdr's server stop."),
    ..tool(
        "herdr",
        "Herdr TUI",
        "The terminal Herdr: coding agents side by side, run by its own server.",
        Category::Agent,
        "herdr",
        "curl -fsSL https://herdr.dev/install.sh | sh",
    )
};
