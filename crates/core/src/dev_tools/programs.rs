//! Where installers put programs: the folders and nvm every script runs with, and finding a
//! program where the Dev Tools page finds tools.

use std::path::{Path, PathBuf};

/// Put in front of every command: the folders installers use
/// ([`INSTALL_DIRS`]), and nvm, so a tool installed a moment ago is found
/// even before the shell profile picks it up.
/// pnpm's global installs need `PNPM_HOME`, which `pnpm setup` exports in
/// `~/.zshrc` (macOS's default folder when it's unset here).
pub(super) const PRELUDE: &str = r#"export PATH="$HOME/.local/bin:$HOME/.claude/local:$HOME/.npm-global/bin:$HOME/.cargo/bin:$HOME/.bun/bin:$HOME/.deno/bin:$HOME/.fly/bin:$HOME/.opencode/bin:$HOME/.amp/bin:$HOME/Library/pnpm:$HOME/.yarn/bin:/opt/homebrew/bin:/usr/local/bin:$PATH"
export NVM_DIR="${NVM_DIR:-$HOME/.nvm}"
export PNPM_HOME="${PNPM_HOME:-$HOME/Library/pnpm}"
[ -s "$NVM_DIR/nvm.sh" ] && . "$NVM_DIR/nvm.sh" >/dev/null 2>&1
"#;

/// The folders installers put programs in. A login shell's `PATH`, which
/// the app adopts, often misses them: their installers add them in
/// `~/.zshrc`, which only interactive shells read.
pub(super) const INSTALL_DIRS: [&str; 11] = [
    "~/.local/bin",
    "~/.claude/local",
    "~/.npm-global/bin",
    "~/.cargo/bin",
    "~/.bun/bin",
    "~/.deno/bin",
    "~/.fly/bin",
    "~/.opencode/bin",
    "~/.amp/bin",
    "~/Library/pnpm",
    // Yarn 1's global packages.
    "~/.yarn/bin",
];

/// Homebrew's folders, searched after nvm's Node.js: a terminal with nvm
/// set up runs nvm's `npx` before Homebrew's.
pub(super) const SYSTEM_DIRS: [&str; 2] = ["/opt/homebrew/bin", "/usr/local/bin"];

/// Finds the program `name` where the Dev Tools page finds tools: on
/// `PATH`, then in the folders installers use, nvm's default Node.js
/// (`npx`, and CLIs installed with npm) and Homebrew's. `cap` is only Cap's
/// CLI.
pub fn find_program(name: &str) -> Option<PathBuf> {
    let home = PathBuf::from(std::env::var("HOME").unwrap_or_default());
    let nvm = std::env::var_os("NVM_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".nvm"));
    let system = SYSTEM_DIRS.map(PathBuf::from);
    find_program_in(
        name,
        std::env::var_os("PATH").as_deref(),
        &home,
        &nvm,
        &system,
    )
}

pub(super) fn find_program_in(
    name: &str,
    path: Option<&std::ffi::OsStr>,
    home: &Path,
    nvm: &Path,
    system: &[PathBuf],
) -> Option<PathBuf> {
    let on_path = path.map(std::env::split_paths).into_iter().flatten();
    on_path
        .chain(INSTALL_DIRS.iter().map(|d| expand_in(d, home)))
        .chain(nvm_default_bin(nvm))
        .chain(system.iter().cloned())
        .map(|dir| dir.join(name))
        .find(|p| is_program(p) && (name != "cap" || is_cap_cli(p)))
}

fn is_program(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt as _;
    std::fs::metadata(path).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
}

/// The `bin` folder of the Node.js nvm's `default` alias names, or of the
/// newest one installed when it names none that is.
fn nvm_default_bin(nvm: &Path) -> Option<PathBuf> {
    let versions = nvm.join("versions/node");
    let mut installed: Vec<(Vec<u64>, std::ffi::OsString)> = std::fs::read_dir(&versions)
        .ok()?
        .filter_map(|e| {
            let name = e.ok()?.file_name();
            Some((node_version(name.to_str()?)?, name))
        })
        .collect();
    installed.sort();
    let wanted = nvm_alias(nvm, "default", 0);
    let (_, name) = wanted
        .and_then(|w| installed.iter().rev().find(|(v, _)| v.starts_with(&w)))
        .or(installed.last())?;
    Some(versions.join(name).join("bin"))
}

/// The version nvm's alias `name` points at, as far as it says
/// (`22` → `[22]`), following aliases of aliases (`lts/*` → `lts/jod` →
/// `v22.11.0`); `None` for the newest (`node`, `stable`, unknown).
fn nvm_alias(nvm: &Path, name: &str, depth: u8) -> Option<Vec<u64>> {
    if depth > 8 {
        return None;
    }
    let text = std::fs::read_to_string(nvm.join("alias").join(name)).ok();
    let target = match (text, name) {
        (Some(text), _) => text.trim().to_string(),
        // An nvm without the `lts/*` file: its newest LTS.
        (None, "lts/*") => {
            return std::fs::read_dir(nvm.join("alias/lts"))
                .ok()?
                .filter_map(|e| {
                    let lts = format!("lts/{}", e.ok()?.file_name().to_str()?);
                    nvm_alias(nvm, &lts, depth + 1)
                })
                .max();
        }
        (None, _) => return None,
    };
    node_version(&target).or_else(|| nvm_alias(nvm, &target, depth + 1))
}

/// `v22.11.0`, `22.11` or `22` as numbers.
fn node_version(text: &str) -> Option<Vec<u64>> {
    let digits = text.strip_prefix('v').unwrap_or(text);
    digits.split('.').map(|p| p.parse().ok()).collect()
}

pub(super) fn expand_in(path: &str, home: &Path) -> PathBuf {
    match path.strip_prefix("~/") {
        Some(rest) => home.join(rest),
        None => PathBuf::from(path),
    }
}

/// Whether `cap` at `path` is Cap's CLI: its installer links it to
/// `Cap.app/Contents/MacOS/cap-cli`. Capistrano's `cap` is a Ruby script.
pub fn is_cap_cli(path: &Path) -> bool {
    std::fs::canonicalize(path).is_ok_and(|real| {
        real.file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.starts_with("cap-cli"))
    })
}
