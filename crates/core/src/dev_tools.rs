//! Developer tools that install with a one-line cURL script (Claude Code,
//! Bun, uv, …): the catalog, which of them are already installed, the order
//! to install a selection in (prerequisites first), the latest released
//! versions, and running an installer, upgrade or uninstaller while
//! streaming its output.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use tokio::io::{AsyncBufReadExt, AsyncRead, BufReader};

use crate::package_manager::PackageManager;
use crate::{CoreError, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Category {
    PackageManager,
    Runtime,
    Agent,
    Other,
}

impl Category {
    pub const ALL: [Category; 4] = [
        Category::PackageManager,
        Category::Runtime,
        Category::Agent,
        Category::Other,
    ];

    pub fn title(self) -> &'static str {
        match self {
            Category::PackageManager => "PACKAGE MANAGERS",
            Category::Runtime => "LANGUAGES & RUNTIMES",
            Category::Agent => "AI AGENTS",
            Category::Other => "TERMINAL & APPS",
        }
    }
}

#[derive(Debug)]
pub struct Tool {
    /// Stable id, also the logo's key.
    pub id: &'static str,
    pub name: &'static str,
    pub description: &'static str,
    pub category: Category,
    /// The command that shows it is installed; empty for an app that has
    /// none, found at its `also` paths only.
    pub bin: &'static str,
    /// Where it lands when the installer doesn't put it on `PATH` (`~` is
    /// `$HOME`).
    pub also: &'static [&'static str],
    /// Besides the `also` paths themselves, the folders (or files) the
    /// installer puts it in, matched by the path found or, for a link, the
    /// path it leads to; with a leading `*`, a part of that real path. A copy
    /// found anywhere else was installed another way (Homebrew's cask of an
    /// app, pyenv's Python), and `uninstall` isn't run on it.
    pub owns: &'static [&'static str],
    /// The install command, run by the login shell.
    pub install: &'static str,
    /// The command that removes what `install` put in place, when there is
    /// one (npm ships with Node.js, so it has none).
    pub uninstall: Option<&'static str>,
    /// Ids of tools that must be installed first.
    pub needs: &'static [&'static str],
    /// Asks for the user's password, so it runs in the terminal instead.
    pub interactive: bool,
    /// `bin --version` prints something useful (and returns quickly); for an
    /// app bundle found, its `Info.plist` gives the version instead.
    pub version: bool,
    /// The Homebrew cask that installs it, an app: the copy found is
    /// Homebrew's, and `uninstall` runs on it, only while Homebrew's Caskroom
    /// has the cask. A copy dragged in from a disk image is left alone.
    pub cask: Option<&'static str>,
    /// Where its latest release is published.
    pub latest: Latest,
    /// Updates an installed copy; `install` when `None`.
    pub upgrade: Option<&'static str>,
}

/// Where a tool's latest version can be read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Latest {
    None,
    /// An npm package name.
    Npm(&'static str),
    /// A GitHub `owner/repo`; its latest release's tag.
    GitHub(&'static str),
    /// A Homebrew cask's token: the version Homebrew's API has for it, which
    /// is what `brew upgrade --cask` installs.
    Cask(&'static str),
}

impl Tool {
    /// The command that brings an installed copy up to date.
    pub fn upgrade_command(&self) -> &'static str {
        self.upgrade.unwrap_or(self.install)
    }
}

const fn tool(
    id: &'static str,
    name: &'static str,
    description: &'static str,
    category: Category,
    bin: &'static str,
    install: &'static str,
) -> Tool {
    Tool {
        id,
        name,
        description,
        category,
        bin,
        also: &[],
        owns: &[],
        install,
        uninstall: None,
        needs: &[],
        interactive: false,
        version: true,
        cask: None,
        latest: Latest::None,
        upgrade: None,
    }
}

pub static TOOLS: &[Tool] = &[
    Tool {
        latest: Latest::GitHub("Homebrew/brew"),
        upgrade: Some("brew update"),
        uninstall: Some(
            r#"/bin/bash -c "$(curl -fsSL https://raw.githubusercontent.com/Homebrew/install/HEAD/uninstall.sh)""#,
        ),
        interactive: true,
        also: &["/opt/homebrew/bin/brew", "/usr/local/bin/brew"],
        ..tool(
            "homebrew",
            "Homebrew",
            "The package manager for macOS. Asks for your password, so it runs in the terminal.",
            Category::PackageManager,
            "brew",
            r#"/bin/bash -c "$(curl -fsSL https://raw.githubusercontent.com/Homebrew/install/HEAD/install.sh)""#,
        )
    },
    Tool {
        owns: &["~/.nvm"],
        uninstall: Some(r#"rm -rf "$NVM_DIR""#),
        also: &["~/.nvm/nvm.sh"],
        version: false,
        ..tool(
            "nvm",
            "nvm",
            "Node Version Manager: installs and switches Node.js versions.",
            Category::PackageManager,
            "nvm",
            "curl -o- https://raw.githubusercontent.com/nvm-sh/nvm/v0.40.3/install.sh | bash",
        )
    },
    Tool {
        owns: &["~/Library/pnpm"],
        latest: Latest::Npm("pnpm"),
        upgrade: Some("pnpm self-update"),
        uninstall: Some(r#"rm -rf "$HOME/Library/pnpm""#),
        also: &["~/Library/pnpm/pnpm"],
        ..tool(
            "pnpm",
            "pnpm",
            "Fast, disk-efficient package manager for JavaScript.",
            Category::PackageManager,
            "pnpm",
            "curl -fsSL https://get.pnpm.io/install.sh | sh -",
        )
    },
    Tool {
        latest: Latest::Npm("npm"),
        needs: &["node"],
        ..tool(
            "npm",
            "npm (latest)",
            "Ships with Node.js; this updates it to the newest release.",
            Category::PackageManager,
            "npm",
            "npm install -g npm@latest",
        )
    },
    Tool {
        latest: Latest::GitHub("astral-sh/uv"),
        upgrade: Some("uv self update"),
        uninstall: Some(
            r#"uv cache clean || true
rm -rf "$(uv python dir)" "$(uv tool dir)"
rm -f "$HOME/.local/bin/uv" "$HOME/.local/bin/uvx""#,
        ),
        also: &["~/.local/bin/uv"],
        ..tool(
            "uv",
            "uv",
            "Python package and project manager from Astral, written in Rust.",
            Category::PackageManager,
            "uv",
            "curl -LsSf https://astral.sh/uv/install.sh | sh",
        )
    },
    Tool {
        owns: &["~/.nvm/versions/node"],
        uninstall: Some(
            r#"nvm deactivate >/dev/null 2>&1 || true
nvm uninstall "$(nvm version default)"
nvm unalias default"#,
        ),
        needs: &["nvm"],
        ..tool(
            "node",
            "Node.js",
            "The latest LTS release, installed and set as default with nvm.",
            Category::Runtime,
            "node",
            "nvm install --lts && nvm alias default 'lts/*'",
        )
    },
    Tool {
        owns: &["~/.bun"],
        latest: Latest::GitHub("oven-sh/bun"),
        upgrade: Some("bun upgrade"),
        uninstall: Some(r#"rm -rf "$HOME/.bun""#),
        also: &["~/.bun/bin/bun"],
        ..tool(
            "bun",
            "Bun",
            "JavaScript runtime, bundler, test runner and package manager.",
            Category::Runtime,
            "bun",
            "curl -fsSL https://bun.sh/install | bash",
        )
    },
    Tool {
        owns: &["~/.deno"],
        latest: Latest::GitHub("denoland/deno"),
        upgrade: Some("deno upgrade"),
        uninstall: Some(r#"rm -rf "$HOME/.deno""#),
        also: &["~/.deno/bin/deno"],
        ..tool(
            "deno",
            "Deno",
            "Secure JavaScript and TypeScript runtime.",
            Category::Runtime,
            "deno",
            "curl -fsSL https://deno.land/install.sh | sh -s -- -y",
        )
    },
    Tool {
        owns: &["~/.local/share/uv/python"],
        uninstall: Some("uv python uninstall --all"),
        needs: &["uv"],
        also: &["~/.local/bin/python3"],
        ..tool(
            "python",
            "Python",
            "The latest CPython, managed by uv and linked as python3.",
            Category::Runtime,
            "python3",
            "uv python install --default",
        )
    },
    Tool {
        owns: &["~/.cargo/bin", "~/.rustup"],
        latest: Latest::GitHub("rust-lang/rust"),
        upgrade: Some("rustup update stable"),
        uninstall: Some("rustup self uninstall -y"),
        also: &["~/.cargo/bin/rustc"],
        ..tool(
            "rust",
            "Rust",
            "rustup with the stable toolchain: rustc, cargo and friends.",
            Category::Runtime,
            "rustc",
            "curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y",
        )
    },
    Tool {
        owns: &["~/.local/share/claude"],
        latest: Latest::Npm("@anthropic-ai/claude-code"),
        upgrade: Some("claude update"),
        uninstall: Some(
            r#"rm -f "$HOME/.local/bin/claude"
rm -rf "$HOME/.local/share/claude""#,
        ),
        also: &["~/.local/bin/claude"],
        ..tool(
            "claude-code",
            "Claude Code",
            "Anthropic's coding agent. The native installer needs no Node.js.",
            Category::Agent,
            "claude",
            "curl -fsSL https://claude.ai/install.sh | bash",
        )
    },
    Tool {
        latest: Latest::Npm("@openai/codex"),
        uninstall: Some(r#"rm -f "$HOME/.local/bin/codex""#),
        also: &["~/.local/bin/codex"],
        ..tool(
            "codex",
            "Codex CLI",
            "OpenAI's coding agent for the terminal.",
            Category::Agent,
            "codex",
            "curl -fsSL https://chatgpt.com/codex/install.sh | CODEX_NON_INTERACTIVE=1 sh",
        )
    },
    Tool {
        uninstall: Some(r#"rm -f "$HOME/.local/bin/omp""#),
        also: &["~/.local/bin/omp"],
        ..tool(
            "omp",
            "oh-my-pi",
            "The omp coding agent: LSP, debugger and IDE tools in the terminal.",
            Category::Agent,
            "omp",
            "curl -fsSL https://omp.sh/install | sh",
        )
    },
    Tool {
        owns: &["~/.opencode"],
        latest: Latest::Npm("opencode-ai"),
        upgrade: Some("opencode upgrade"),
        uninstall: Some(r#"rm -rf "$HOME/.opencode""#),
        also: &["~/.opencode/bin/opencode"],
        ..tool(
            "opencode",
            "OpenCode",
            "Open source coding agent with a terminal UI.",
            Category::Agent,
            "opencode",
            "curl -fsSL https://opencode.ai/install | bash",
        )
    },
    Tool {
        owns: &["~/.amp"],
        latest: Latest::Npm("@sourcegraph/amp"),
        uninstall: Some(
            r#"rm -f "$HOME/.local/bin/amp"
rm -rf "$HOME/.amp""#,
        ),
        also: &["~/.local/bin/amp", "~/.amp/bin/amp"],
        ..tool(
            "amp",
            "Amp",
            "Sourcegraph's agentic coding tool.",
            Category::Agent,
            "amp",
            "curl -fsSL https://ampcode.com/install.sh | bash",
        )
    },
    Tool {
        owns: &["~/.local/share/cursor-agent"],
        uninstall: Some(
            r#"rm -f "$HOME/.local/bin/cursor-agent" "$HOME/.local/bin/agent"
rm -rf "$HOME/.local/share/cursor-agent""#,
        ),
        also: &["~/.local/bin/cursor-agent"],
        ..tool(
            "cursor",
            "Cursor CLI",
            "Cursor's agent in the terminal (cursor-agent).",
            Category::Agent,
            "cursor-agent",
            "curl https://cursor.com/install -fsS | bash",
        )
    },
    Tool {
        uninstall: Some(r#"rm -f "$HOME/.local/bin/droid""#),
        also: &["~/.local/bin/droid"],
        ..tool(
            "droid",
            "Factory Droid",
            "Factory's software development agent.",
            Category::Agent,
            "droid",
            "curl -fsSL https://app.factory.ai/cli | sh",
        )
    },
    Tool {
        uninstall: Some(r#"rm -f "$HOME/.local/bin/kiro-cli""#),
        also: &["~/.local/bin/kiro-cli"],
        ..tool(
            "kiro",
            "Kiro CLI",
            "AWS's agentic coding assistant for the terminal.",
            Category::Agent,
            "kiro-cli",
            "curl -fsSL https://cli.kiro.dev/install | bash",
        )
    },
    Tool {
        latest: Latest::Npm("@google/gemini-cli"),
        uninstall: Some("npm uninstall -g @google/gemini-cli"),
        needs: &["node"],
        ..tool(
            "gemini",
            "Gemini CLI",
            "Google's open source agent, installed with npm.",
            Category::Agent,
            "gemini",
            "npm install -g @google/gemini-cli",
        )
    },
    Tool {
        // Where its installer links the cap-cli of Cap.app.
        owns: &["~/.local/bin/cap", "~/.cap/bin/cap"],
        uninstall: Some(
            r#"for shim in "$HOME/.local/bin/cap" "$HOME/.cap/bin/cap"; do
  case "$(readlink "$shim")" in */cap-cli*) rm -f "$shim" ;; esac
done
rm -rf "/Applications/Cap.app" "$HOME/Applications/Cap.app""#,
        ),
        also: &["/Applications/Cap.app", "~/Applications/Cap.app"],
        version: false,
        ..tool(
            "cap",
            "Cap",
            "Open source screen recorder: the desktop app and its cap CLI.",
            Category::Other,
            "cap",
            "curl -fsSL https://cap.so/install-cli.sh | sh",
        )
    },
    Tool {
        // Where its installer links the app's CLI.
        owns: &["~/.local/bin/zed"],
        latest: Latest::GitHub("zed-industries/zed"),
        uninstall: Some(
            r#"rm -f "$HOME/.local/bin/zed"
rm -rf "/Applications/Zed.app" "$HOME/Applications/Zed.app""#,
        ),
        also: &["/Applications/Zed.app", "~/Applications/Zed.app"],
        ..tool(
            "zed",
            "Zed",
            "High-performance code editor, with the zed command.",
            Category::Other,
            "zed",
            "curl -f https://zed.dev/install.sh | sh",
        )
    },
    Tool {
        // The app only: its README installs it with Homebrew's cask (or a signed, notarized disk
        // image from GitHub Releases, dragged into /Applications). Its `puremac` command is
        // another program, the PureMac CLI from momenbasel/tap/puremac-cli.
        cask: Some("puremac"),
        latest: Latest::Cask("puremac"),
        upgrade: Some("brew upgrade --cask puremac"),
        uninstall: Some("brew uninstall --cask puremac"),
        needs: &["homebrew"],
        also: &["/Applications/PureMac.app", "~/Applications/PureMac.app"],
        ..tool(
            "puremac",
            "PureMac",
            "Free, open source Mac cleaner and app uninstaller with no telemetry, from its Homebrew cask.",
            Category::Other,
            "",
            "brew install --cask puremac",
        )
    },
    Tool {
        latest: Latest::GitHub("starship/starship"),
        uninstall: Some(r#"rm -f "$HOME/.local/bin/starship""#),
        also: &["~/.local/bin/starship"],
        ..tool(
            "starship",
            "Starship",
            "Fast, customizable prompt for any shell.",
            Category::Other,
            "starship",
            r#"curl -sS https://starship.rs/install.sh | sh -s -- -y -b "$HOME/.local/bin""#,
        )
    },
    Tool {
        owns: &["~/.fly"],
        latest: Latest::GitHub("superfly/flyctl"),
        upgrade: Some("flyctl version upgrade"),
        uninstall: Some(r#"rm -rf "$HOME/.fly""#),
        also: &["~/.fly/bin/flyctl"],
        ..tool(
            "flyio",
            "Fly.io CLI",
            "flyctl: deploy apps to Fly.io.",
            Category::Other,
            "flyctl",
            "curl -L https://fly.io/install.sh | sh",
        )
    },
    Tool {
        latest: Latest::Npm("cf"),
        uninstall: Some("npm uninstall -g cf"),
        needs: &["node"],
        ..tool(
            "cf",
            "Cloudflare CLI",
            "cf (beta): the whole Cloudflare API and Workers projects from the terminal, installed with npm.",
            Category::Other,
            "cf",
            "npm install -g cf",
        )
    },
];

/// The npm package a tool installs with `npm install -g` (Gemini CLI, the
/// Cloudflare CLI): those install with whichever package manager is picked.
/// Not npm's own update.
pub fn npm_package(tool: &Tool) -> Option<&'static str> {
    let package = tool.install.strip_prefix("npm install -g ")?;
    (tool.id != "npm" && !package.contains(char::is_whitespace)).then_some(package)
}

/// `tool`'s install command with `pm` (`npm install -g x` → `bun add -g
/// x`); the catalog's own for a tool npm doesn't install, or without `pm`.
pub fn install_command(tool: &Tool, pm: Option<PackageManager>) -> String {
    match (npm_package(tool), pm) {
        (Some(package), Some(pm)) => pm.global_install(package),
        _ => tool.install.to_string(),
    }
}

/// `tool`'s upgrade command with `pm`, like [`install_command`]: installing
/// the package again brings the latest.
pub fn upgrade_command(tool: &Tool, pm: Option<PackageManager>) -> String {
    match (npm_package(tool), pm) {
        (Some(package), Some(pm)) => pm.global_install(package),
        _ => tool.upgrade_command().to_string(),
    }
}

pub fn find(id: &str) -> Option<&'static Tool> {
    TOOLS.iter().find(|t| t.id == id)
}

/// The order to install `wanted` in: every missing prerequisite before the
/// tools that need it, each tool once. Installed tools are left out.
pub fn plan(wanted: &[&str], installed: &dyn Fn(&str) -> bool) -> Vec<&'static Tool> {
    fn visit(id: &str, installed: &dyn Fn(&str) -> bool, out: &mut Vec<&'static Tool>) {
        let Some(tool) = find(id) else { return };
        if out.iter().any(|t| t.id == id) {
            return;
        }
        for need in tool.needs {
            if !installed(need) {
                visit(need, installed, out);
            }
        }
        out.push(tool);
    }
    let mut out = Vec::new();
    for id in wanted {
        visit(id, installed, &mut out);
    }
    out
}

/// Put in front of every command: the folders installers use
/// ([`INSTALL_DIRS`]), and nvm, so a tool installed a moment ago is found
/// even before the shell profile picks it up.
/// pnpm's global installs need `PNPM_HOME`, which `pnpm setup` exports in
/// `~/.zshrc` (macOS's default folder when it's unset here).
const PRELUDE: &str = r#"export PATH="$HOME/.local/bin:$HOME/.claude/local:$HOME/.npm-global/bin:$HOME/.cargo/bin:$HOME/.bun/bin:$HOME/.deno/bin:$HOME/.fly/bin:$HOME/.opencode/bin:$HOME/.amp/bin:$HOME/Library/pnpm:$HOME/.yarn/bin:/opt/homebrew/bin:/usr/local/bin:$PATH"
export NVM_DIR="${NVM_DIR:-$HOME/.nvm}"
export PNPM_HOME="${PNPM_HOME:-$HOME/Library/pnpm}"
[ -s "$NVM_DIR/nvm.sh" ] && . "$NVM_DIR/nvm.sh" >/dev/null 2>&1
"#;

/// The folders installers put programs in. A login shell's `PATH`, which
/// the app adopts, often misses them: their installers add them in
/// `~/.zshrc`, which only interactive shells read.
const INSTALL_DIRS: [&str; 11] = [
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
const SYSTEM_DIRS: [&str; 2] = ["/opt/homebrew/bin", "/usr/local/bin"];

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

fn find_program_in(
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

/// A login shell to run a script in: the user's own when it runs POSIX
/// scripts, so the profile's `PATH` and variables apply, else zsh
/// ([`crate::probe::script_shell`]). `login` is `$SHELL`.
fn shell(login: Option<&str>) -> tokio::process::Command {
    let mut cmd = tokio::process::Command::new(crate::probe::script_shell(login));
    cmd.arg("-l")
        .stdin(Stdio::null())
        .env("NONINTERACTIVE", "1")
        .kill_on_drop(true);
    // A new session has no controlling terminal, so an installer that
    // reads /dev/tty fails fast instead of waiting for an answer.
    #[cfg(unix)]
    unsafe {
        cmd.pre_exec(|| {
            libc::setsid();
            Ok(())
        });
    }
    cmd
}

fn expand_in(path: &str, home: &Path) -> PathBuf {
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    pub path: PathBuf,
    pub version: Option<String>,
    /// For an app whose copy isn't the one its own commands install,
    /// upgrade and remove (dragged in from a disk image, kept up to date by
    /// its own updater): what it was installed outside of, said instead of
    /// offering Upgrade, Reinstall or Uninstall. `None` for every other copy
    /// ([`installed_outside`]).
    pub outside: Option<Outside>,
}

/// What an app's copy was installed outside of ([`Found::outside`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outside {
    /// A cask's app that Homebrew's Caskroom doesn't have: dragged in from
    /// its disk image.
    Homebrew,
    /// Another app with no command of its install script leading into it.
    InstallScript,
}

impl Outside {
    /// What it was installed outside of: "Homebrew", "its install script".
    pub fn of(self) -> &'static str {
        match self {
            Outside::Homebrew => "Homebrew",
            Outside::InstallScript => "its install script",
        }
    }

    /// "Installed outside Homebrew", for the page.
    pub fn text(self) -> String {
        format!("Installed outside {}", self.of())
    }
}

const MARK: &str = "__SAWIT_TOOL__";

/// How long detection waits for the login shell.
const DETECT_TIMEOUT: Duration = Duration::from_secs(20);

/// What [`detect`] found.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Detection {
    pub found: HashMap<&'static str, Found>,
    /// Why the login shell couldn't say which tools it finds, when it
    /// couldn't. `found` then has only the tools at their usual places, so
    /// a tool missing from it may still be installed.
    pub problem: Option<String>,
}

/// Which tools are installed: `command -v` in one login shell, then the
/// known install locations, then each one's `--version` (an app's
/// `Info.plist`).
pub async fn detect() -> Detection {
    detect_as(crate::probe::login_shell().as_deref()).await
}

/// The script detection runs in the login shell: a line with the path of
/// each tool's command, or an empty one. An app without a command (PureMac)
/// has no line: it's found at its `also` paths only.
fn detect_script() -> String {
    let mut script = String::from(PRELUDE);
    for t in TOOLS.iter().filter(|t| !t.bin.is_empty()) {
        // nvm is a shell function; its path is the script that defines it.
        let probe = if t.id == "nvm" {
            r#"[ -s "$NVM_DIR/nvm.sh" ] && printf '%s' "$NVM_DIR/nvm.sh""#.to_string()
        } else if t.id == "python" {
            // macOS's /usr/bin/python3 is the old system Python (or a stub
            // that asks to install the Xcode tools); it doesn't count.
            "/usr/bin/which -a python3 2>/dev/null | grep -v '^/usr/bin/' | head -n 1".to_string()
        } else {
            format!("command -v {} 2>/dev/null", t.bin)
        };
        script.push_str(&format!("printf '{MARK}{}\\t%s\\n' \"$({probe})\"\n", t.id));
    }
    script
}

/// The first of `tool`'s `also` paths that is there, under `home`.
fn at_known_place(tool: &Tool, home: &Path) -> Option<PathBuf> {
    tool.also
        .iter()
        .map(|p| expand_in(p, home))
        .find(|p| p.exists())
}

/// The version of `tool`'s copy at `path`: an app bundle's from its
/// `Info.plist`, else the number its `--version` prints.
async fn version_of(tool: &Tool, path: &Path) -> Option<String> {
    if !tool.version {
        return None;
    }
    if is_app(path) {
        return app_version(path).await;
    }
    let out = crate::cmd::run(path, ["--version"], Duration::from_secs(3))
        .await
        .ok()?;
    let text = if out.stdout.trim().is_empty() {
        out.stderr
    } else {
        out.stdout
    };
    short_version(&text)
}

/// [`detect`] with `login` as `$SHELL`.
async fn detect_as(login: Option<&str>) -> Detection {
    let mut cmd = shell(login);
    cmd.arg("-c")
        .arg(detect_script())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let shell_name = crate::probe::script_shell(login).display().to_string();
    let (out, problem) = match tokio::time::timeout(DETECT_TIMEOUT, cmd.output()).await {
        Ok(Ok(out)) => {
            let text = String::from_utf8_lossy(&out.stdout).into_owned();
            // It printed a line per tool, unless it failed before the first.
            let problem = (!text.contains(MARK)).then(|| {
                let why = String::from_utf8_lossy(&out.stderr)
                    .lines()
                    .rev()
                    .find(|l| !l.trim().is_empty())
                    .map(|l| format!(": {}", l.trim()))
                    .unwrap_or_default();
                format!(
                    "{shell_name} -l failed (exit {}){why}",
                    out.status
                        .code()
                        .map_or_else(|| "signal".to_string(), |c| c.to_string())
                )
            });
            (text, problem)
        }
        Ok(Err(e)) => (
            String::new(),
            Some(format!("{shell_name} couldn't start: {e}")),
        ),
        Err(_) => (
            String::new(),
            Some(format!(
                "{shell_name} -l didn't answer within {} seconds",
                DETECT_TIMEOUT.as_secs()
            )),
        ),
    };
    let mut paths: HashMap<&'static str, PathBuf> = HashMap::new();
    for line in out.lines() {
        let Some((id, path)) = line
            .split_once(MARK)
            .and_then(|(_, rest)| rest.split_once('\t'))
        else {
            continue;
        };
        // `command -v` names functions and aliases without a slash.
        if let Some(t) = find(id)
            && path.starts_with('/')
            // Another program by the same name (Capistrano's `cap`).
            && (t.id != "cap" || is_cap_cli(Path::new(path)))
        {
            paths.insert(t.id, PathBuf::from(path));
        }
    }
    let home = PathBuf::from(std::env::var("HOME").unwrap_or_default());
    for t in TOOLS {
        if paths.contains_key(t.id) {
            continue;
        }
        if let Some(p) = at_known_place(t, &home) {
            paths.insert(t.id, p);
        }
    }
    let versions = futures::future::join_all(
        paths
            .iter()
            .map(|(id, path)| async move { Some((*id, version_of(find(id)?, path).await?)) }),
    )
    .await;
    let mut versions: HashMap<&str, String> = versions.into_iter().flatten().collect();
    let found = paths
        .into_iter()
        .map(|(id, path)| {
            let version = versions.remove(id);
            let outside = find(id).and_then(|t| installed_outside(t, &path));
            (
                id,
                Found {
                    path,
                    version,
                    outside,
                },
            )
        })
        .collect();
    Detection { found, problem }
}

/// The version number in a `--version` line ("Python 3.13.2" → "3.13.2").
fn short_version(text: &str) -> Option<String> {
    let first = text.lines().find(|l| !l.trim().is_empty())?;
    let word = first
        .split(|c: char| c.is_whitespace() || c == ',' || c == '(' || c == ')')
        .map(|w| w.trim_start_matches('v'))
        .find(|w| w.chars().next().is_some_and(|c| c.is_ascii_digit()) && w.contains('.'))?;
    Some(word.to_string())
}

fn is_app(path: &Path) -> bool {
    path.extension().is_some_and(|e| e == "app")
}

/// The version an app bundle gives in its `Info.plist`
/// (`CFBundleShortVersionString`, "3.2.0"), read with `plutil`, which reads
/// the XML and binary forms alike; `None` without one.
async fn app_version(app: &Path) -> Option<String> {
    let plist = app.join("Contents/Info.plist");
    let out = crate::cmd::run(
        "/usr/bin/plutil",
        [
            std::ffi::OsStr::new("-extract"),
            "CFBundleShortVersionString".as_ref(),
            "raw".as_ref(),
            "-o".as_ref(),
            "-".as_ref(),
            plist.as_os_str(),
        ],
        Duration::from_secs(3),
    )
    .await
    .ok()?;
    if out.code != Some(0) {
        return None;
    }
    let version = out.stdout.trim();
    version
        .starts_with(|c: char| c.is_ascii_digit())
        .then(|| version.to_string())
}

/// The version Homebrew's API gives a cask
/// (`formulae.brew.sh/api/cask/<token>.json`): "3.2.0", the part before a
/// build number ("1.2.3,456" → "1.2.3"); `None` for `latest`, which has none.
fn cask_version(json: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(json).ok()?;
    let version = v["version"].as_str()?.split(',').next()?.trim();
    version
        .starts_with(|c: char| c.is_ascii_digit())
        .then(|| version.to_string())
}

/// The version in a release tag ("v1.2.3", "bun-v1.2.3" → "1.2.3").
fn tag_version(tag: &str) -> Option<String> {
    let start = tag.find(|c: char| c.is_ascii_digit())?;
    Some(tag[start..].trim().to_string())
}

/// `tool`'s latest released version, or `None` when it can't be read.
async fn latest_version(tool: &Tool) -> Option<String> {
    let (args, url): (&[&str], String) = match tool.latest {
        Latest::None => return None,
        Latest::Npm(pkg) => (
            &["-fsSL"],
            format!("https://registry.npmjs.org/{pkg}/latest"),
        ),
        // The release page redirects to its tag; no rate-limited API call.
        Latest::GitHub(repo) => (
            &["-fsSLI", "-o", "/dev/null", "-w", "%{url_effective}"],
            format!("https://github.com/{repo}/releases/latest"),
        ),
        Latest::Cask(token) => (
            &["-fsSL"],
            format!("https://formulae.brew.sh/api/cask/{token}.json"),
        ),
    };
    let out = crate::cmd::run(
        "/usr/bin/curl",
        args.iter()
            .copied()
            .chain(["--max-time", "8", url.as_str()]),
        Duration::from_secs(10),
    )
    .await
    .ok()?;
    if out.code != Some(0) {
        return None;
    }
    match tool.latest {
        Latest::Npm(_) => {
            let v: serde_json::Value = serde_json::from_str(&out.stdout).ok()?;
            Some(v["version"].as_str()?.to_string())
        }
        Latest::Cask(_) => cask_version(&out.stdout),
        _ => tag_version(out.stdout.trim().rsplit_once("/tag/")?.1),
    }
}

/// The latest released version of every tool that publishes one, fetched
/// at once; tools that fail are left out.
pub async fn latest_versions() -> HashMap<&'static str, String> {
    futures::future::join_all(
        TOOLS
            .iter()
            .map(|t| async move { Some((t.id, latest_version(t).await?)) }),
    )
    .await
    .into_iter()
    .flatten()
    .collect()
}

/// The newer release installed tool `id` can upgrade to. None for an app
/// whose copy its upgrade command doesn't upgrade ([`Found::outside`]): its
/// version shows, but nothing offers to upgrade it.
pub fn newer<'a>(
    id: &str,
    found: &HashMap<&'static str, Found>,
    latest: &'a HashMap<&'static str, String>,
) -> Option<&'a str> {
    let found = found.get(id)?;
    if found.outside.is_some() {
        return None;
    }
    let installed = found.version.as_ref()?;
    let latest = latest.get(id)?;
    is_newer(latest, installed).then_some(latest.as_str())
}

/// The tools upgrading `wanted` runs, in catalog order: every installed
/// tool with a newer release when `wanted` is empty, else each wanted tool
/// that is installed and not known to be up to date. Never an app whose
/// copy its upgrade command doesn't upgrade ([`Found::outside`]), named or
/// not: the page, `upgrade_dev_tools` and `sawit pupuk` all leave it alone.
pub fn upgrade_plan(
    wanted: &[&str],
    found: &HashMap<&'static str, Found>,
    latest: &HashMap<&'static str, String>,
) -> Vec<&'static Tool> {
    TOOLS
        .iter()
        .filter(|t| found.get(t.id).is_some_and(|f| f.outside.is_none()))
        .filter(|t| {
            if wanted.is_empty() {
                newer(t.id, found, latest).is_some()
            } else {
                wanted.contains(&t.id)
                    && (newer(t.id, found, latest).is_some() || !latest.contains_key(t.id))
            }
        })
        .collect()
}

/// `latest` is a higher dotted version than `installed`. Pre-release and
/// build suffixes ("-beta.1", "+abc") are ignored; missing parts count as 0.
pub fn is_newer(latest: &str, installed: &str) -> bool {
    fn parts(v: &str) -> Vec<u64> {
        let core = v.trim().trim_start_matches('v');
        let core = core.split(['-', '+', ' ']).next().unwrap_or_default();
        core.split('.')
            .map_while(|p| {
                let digits: String = p.chars().take_while(|c| c.is_ascii_digit()).collect();
                digits.parse().ok()
            })
            .collect()
    }
    let (a, b) = (parts(latest), parts(installed));
    if a.is_empty() || b.is_empty() {
        return false;
    }
    let n = a.len().max(b.len());
    let at = |v: &[u64], i: usize| v.get(i).copied().unwrap_or(0);
    (0..n)
        .map(|i| at(&a, i).cmp(&at(&b, i)))
        .find(|o| o.is_ne())
        .is_some_and(|o| o.is_gt())
}

/// Runs `tool`'s installer, sending each line it prints to `on_line`.
/// Fails with the last lines of output when it exits non-zero. Dropping
/// the future stops the installer and everything it started.
pub async fn install(tool: &Tool, on_line: impl Fn(String) + Send + Sync) -> Result<()> {
    run_locked(tool.id, tool.name, tool.install, on_line).await
}

/// Runs `command` (`tool`'s installer or upgrade) like [`install`].
pub async fn run(tool: &Tool, command: &str, on_line: impl Fn(String) + Send + Sync) -> Result<()> {
    run_locked(tool.id, tool.name, command, on_line).await
}

/// How to remove the copy of `tool` found at `found`: `brew uninstall`
/// when Homebrew put it there, `npm uninstall -g` when npm did, the tool's
/// own command when its installer did (at one of its known locations, or in
/// the folders it installs to: [`Tool::owns`]), and `None` for anything
/// else (pyenv's Python, Capistrano's `cap`, …), which Sawit leaves alone.
/// A tool with no uninstaller of its own (npm, which comes with Node.js)
/// gets none either: Homebrew's npm is its `node`, and removing it would
/// remove Node.js.
pub fn uninstall_command(tool: &Tool, found: &Path) -> Option<String> {
    let real = std::fs::canonicalize(found).unwrap_or_else(|_| found.to_path_buf());
    let home = PathBuf::from(std::env::var("HOME").unwrap_or_default());
    uninstall_command_in(tool, found, &real, &home)
}

/// Homebrew's Caskroom, which has a folder for each cask installed: on Apple
/// silicon, and on Intel.
const CASKROOMS: [&str; 2] = ["/opt/homebrew/Caskroom", "/usr/local/Caskroom"];

fn uninstall_command_in(tool: &Tool, found: &Path, real: &Path, home: &Path) -> Option<String> {
    uninstall_command_at(tool, found, real, home, &CASKROOMS.map(PathBuf::from))
}

/// [`uninstall_command_in`] with `caskrooms` for Homebrew's Caskroom.
fn uninstall_command_at(
    tool: &Tool,
    found: &Path,
    real: &Path,
    home: &Path,
    caskrooms: &[PathBuf],
) -> Option<String> {
    let own = tool.uninstall?;
    // An app from a Homebrew cask, where the cask puts it: Homebrew's copy while its Caskroom
    // has the cask. Otherwise it was dragged in from a disk image, and `brew uninstall` would
    // fail on it.
    if let Some(token) = tool.cask {
        let at_its_place = tool.also.iter().any(|p| expand_in(p, home) == found);
        let homebrews = caskrooms.iter().any(|room| room.join(token).is_dir());
        return (at_its_place && homebrews).then(|| own.to_string());
    }
    // Another app is its install script's copy when the command the script links (Zed's
    // ~/.local/bin/zed, Cap's shims) leads into it. Without one, it was dragged in from a disk
    // image and keeps itself up to date: running the script again would replace it or put a
    // second copy beside it, and its uninstaller would delete it, so Sawit leaves it alone.
    if is_app(found) {
        let linked = tool.owns.iter().filter(|p| !p.starts_with('*')).any(|p| {
            std::fs::canonicalize(expand_in(p, home))
                .is_ok_and(|to| to.starts_with(found) || to.starts_with(real))
        });
        return linked.then(|| own.to_string());
    }
    let parts: Vec<_> = real
        .components()
        .map(|c| c.as_os_str().to_string_lossy())
        .collect();
    if tool.id != "homebrew"
        && let Some(ix) = parts.iter().position(|p| p == "Cellar")
        && let Some(formula) = parts.get(ix + 1)
    {
        // A folder name, into a shell command: only a formula's.
        let formula_like = formula
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "@._+-".contains(c));
        return formula_like.then(|| format!("brew uninstall {formula}"));
    }
    let real_text = real.to_string_lossy();
    // An npm package installed globally: the package manager that put it
    // there (npm, pnpm, Bun, Yarn) removes it.
    if let Latest::Npm(package) = tool.latest
        && tool.id != "npm"
        && real_text.contains(&format!("/node_modules/{package}/"))
    {
        return Some(PackageManager::owning(&real_text).global_uninstall(package));
    }
    // An `also` path is the copy itself (a command; an app is worked out
    // above). A command linked into an app from elsewhere (Homebrew's cask
    // of Zed links /opt/homebrew/bin/zed) isn't this one.
    let ours = tool.also.iter().any(|p| expand_in(p, home) == found)
        || tool.owns.iter().any(|p| match p.strip_prefix('*') {
            Some(part) => real_text.contains(part),
            None => {
                let p = expand_in(p, home);
                found.starts_with(&p) || real.starts_with(&p)
            }
        });
    ours.then(|| own.to_string())
}

/// Whether `tool` is an app: an app bundle is one of its places (Cap, Zed,
/// PureMac).
fn is_app_tool(tool: &Tool) -> bool {
    tool.also.iter().any(|p| p.ends_with(".app"))
}

/// [`Found::outside`] for the copy of `tool` found at `found`. An app's
/// upgrade (its cask's `brew upgrade`, its install script again) and
/// uninstall act on the copy they put in place, so a copy that isn't that
/// one (by the rule of [`uninstall_command`]) gets neither: outside
/// Homebrew for a cask's app, outside its install script for another. Every
/// other tool's copy is `None`, as before.
pub fn installed_outside(tool: &Tool, found: &Path) -> Option<Outside> {
    let real = std::fs::canonicalize(found).unwrap_or_else(|_| found.to_path_buf());
    let home = PathBuf::from(std::env::var("HOME").unwrap_or_default());
    installed_outside_at(tool, found, &real, &home, &CASKROOMS.map(PathBuf::from))
}

/// [`installed_outside`] with `caskrooms` for Homebrew's Caskroom.
fn installed_outside_at(
    tool: &Tool,
    found: &Path,
    real: &Path,
    home: &Path,
    caskrooms: &[PathBuf],
) -> Option<Outside> {
    if !is_app_tool(tool) || uninstall_command_at(tool, found, real, home, caskrooms).is_some() {
        return None;
    }
    Some(if tool.cask.is_some() {
        Outside::Homebrew
    } else {
        Outside::InstallScript
    })
}

/// What losing a tool it needs does to `dependent`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Impact {
    /// It runs through the tool removed (Gemini CLI through Node.js): it
    /// stops working.
    StopsWorking,
    /// An app from a Homebrew cask (PureMac): it keeps working, but
    /// Homebrew no longer updates it.
    NoHomebrewUpdates,
}

pub fn impact(dependent: &Tool) -> Impact {
    if dependent.cask.is_some() {
        Impact::NoHomebrewUpdates
    } else {
        Impact::StopsWorking
    }
}

/// The warning before removing a tool the installed tools `needed_by` need,
/// as the page, `uninstall_dev_tools` and `list_dev_tools` give it, without
/// its full stop: "npm (latest), Gemini CLI need it and will stop working",
/// "PureMac won't get updates through Homebrew any more", or both joined
/// with "; ". Empty when nothing needs it.
pub fn needed_by_warning(needed_by: &[&Tool]) -> String {
    let names = |which: Impact| -> Vec<&str> {
        needed_by
            .iter()
            .filter(|t| impact(t) == which)
            .map(|t| t.name)
            .collect()
    };
    let mut parts = Vec::new();
    let stop = names(Impact::StopsWorking);
    if !stop.is_empty() {
        let s = if stop.len() == 1 { "s" } else { "" };
        parts.push(format!(
            "{} need{s} it and will stop working",
            stop.join(", ")
        ));
    }
    let apps = names(Impact::NoHomebrewUpdates);
    if !apps.is_empty() {
        parts.push(format!(
            "{} won't get updates through Homebrew any more",
            apps.join(", ")
        ));
    }
    parts.join("; ")
}

/// The package manager that installed the copy of an npm tool found at
/// `found` (its links followed): what upgrades it, so an upgrade doesn't put
/// a second copy beside it with another one.
pub fn installed_with(found: &Path) -> PackageManager {
    let real = std::fs::canonicalize(found).unwrap_or_else(|_| found.to_path_buf());
    PackageManager::owning(&real.to_string_lossy())
}

/// Runs an uninstall `command` for `tool`, like [`install`].
pub async fn uninstall(
    tool: &Tool,
    command: &str,
    on_line: impl Fn(String) + Send + Sync,
) -> Result<()> {
    run_locked(tool.id, tool.name, command, on_line).await
}

/// Whether the uninstall `command` of `tool` asks for the user's password,
/// which only a terminal can answer: Homebrew's own uninstaller does, a
/// `brew uninstall` of one formula doesn't.
pub fn in_terminal(tool: &Tool, command: &str) -> bool {
    tool.interactive && !command.starts_with("brew ")
}

/// How [`uninstall_plan`] removes one tool.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Removal {
    /// Detection found no copy: there is nothing to remove.
    Missing,
    /// The copy found wasn't put there by the tool's installer, Homebrew or
    /// npm (pyenv's Python, Capistrano's `cap`, …): Sawit leaves it alone
    /// ([`uninstall_command`]).
    LeftAlone,
    /// The tool has no uninstaller of its own: it comes with the tools it
    /// needs, named here (npm with Node.js), and goes when they go.
    Bundled(Vec<&'static str>),
    /// Its uninstaller asks for the user's password ([`in_terminal`]): the
    /// command, for the user to run in a terminal.
    Terminal(String),
    /// The command that removes the copy found.
    Run(String),
}

/// One tool of an uninstall, worked out as the Dev Tools page's trash button
/// works it out, for `uninstall_dev_tools` and `sawit cabut`.
#[derive(Debug, Clone)]
pub struct Uninstall {
    pub tool: &'static Tool,
    /// Where detection found it.
    pub found: Option<PathBuf>,
    pub removal: Removal,
    /// The installed tools that need it and stay, so they stop working once
    /// it's gone, or for a cask's app stop getting updates
    /// ([`needed_by_warning`]; the page names them before it asks). Empty
    /// when nothing is removed.
    pub needed_by: Vec<&'static Tool>,
}

/// What uninstalling the tools `wanted` does, given what [`detect`] found:
/// each tool of the catalog once, a tool before the ones it needs (Python's
/// uninstaller is `uv python uninstall`, so uv goes after it; Gemini CLI's
/// is `npm uninstall`, so nvm and Node.js go after it). Ids not in the
/// catalog are left out.
pub fn uninstall_plan(wanted: &[&str], found: &HashMap<&'static str, Found>) -> Vec<Uninstall> {
    uninstall_plan_with(wanted, found, &|tool, path| uninstall_command(tool, path))
}

/// [`uninstall_plan`] with `command` for [`uninstall_command`].
fn uninstall_plan_with(
    wanted: &[&str],
    found: &HashMap<&'static str, Found>,
    command: &dyn Fn(&Tool, &Path) -> Option<String>,
) -> Vec<Uninstall> {
    let mut tools: Vec<&'static Tool> = Vec::new();
    for tool in wanted.iter().filter_map(|id| find(id)) {
        if !tools.iter().any(|t| t.id == tool.id) {
            tools.push(tool);
        }
    }
    // A tool needs fewer tools, directly or through another, than any tool
    // that needs it; the sort keeps the order given otherwise.
    tools.sort_by_key(|t| std::cmp::Reverse(plan(&[t.id], &|_| false).len()));
    let mut out: Vec<Uninstall> = tools
        .into_iter()
        .map(|tool| {
            let path = found.get(tool.id).map(|f| f.path.clone());
            let removal = match &path {
                None => Removal::Missing,
                Some(_) if tool.uninstall.is_none() => Removal::Bundled(
                    tool.needs
                        .iter()
                        .filter_map(|id| find(id))
                        .map(|t| t.name)
                        .collect(),
                ),
                Some(path) => match command(tool, path) {
                    None => Removal::LeftAlone,
                    Some(c) if in_terminal(tool, &c) => Removal::Terminal(c),
                    Some(c) => Removal::Run(c),
                },
            };
            Uninstall {
                tool,
                found: path,
                removal,
                needed_by: Vec::new(),
            }
        })
        .collect();
    let removed: Vec<&str> = out
        .iter()
        .filter(|u| matches!(u.removal, Removal::Run(_) | Removal::Terminal(_)))
        .map(|u| u.tool.id)
        .collect();
    for u in out.iter_mut().filter(|u| removed.contains(&u.tool.id)) {
        // A copy installed outside what the tool needs (PureMac from its disk image) doesn't
        // need it.
        u.needed_by = dependents(u.tool.id)
            .into_iter()
            .filter(|t| {
                found.get(t.id).is_some_and(|f| f.outside.is_none()) && !removed.contains(&t.id)
            })
            .collect();
    }
    out
}

/// Runs `command` for the tool `id` while holding that tool's lock: one
/// installer, upgrade or uninstaller per tool at a time, whether the Dev
/// Tools page, an agent (`install_dev_tools`, `upgrade_dev_tools`,
/// `uninstall_dev_tools`), `sawit pasang`, `sawit pupuk` or `sawit cabut`
/// runs it. Two at once would write the same folders (rustup, nvm's clone,
/// uv's).
async fn run_locked(
    id: &str,
    name: &str,
    command: &str,
    on_line: impl Fn(String) + Send + Sync,
) -> Result<()> {
    let _lock = lock_tool(id, name)?;
    run_script(name, command, on_line).await
}

/// Runs `command` in the login shell as the Dev Tools page runs an installer
/// (its output streamed to `on_line`, stopped with everything it started when
/// dropped), under the lock `id`: what the Skills page runs to install, update
/// or remove a pack. `name` says what is busy when the lock is held.
pub async fn run_shell(
    id: &str,
    name: &str,
    command: &str,
    on_line: impl Fn(String) + Send + Sync,
) -> Result<()> {
    run_locked(id, name, command, on_line).await
}

/// Holds tool `id`'s lock until dropped; fails at once when it's held.
fn lock_tool(id: &str, name: &str) -> Result<std::fs::File> {
    use std::os::fd::AsRawFd as _;
    let dir = std::env::temp_dir().join("sawit-tool-locks");
    std::fs::create_dir_all(&dir)?;
    let file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(dir.join(format!("{id}.lock")))?;
    if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
        let e = std::io::Error::last_os_error();
        if e.raw_os_error() == Some(libc::EWOULDBLOCK) {
            return Err(CoreError::Config(format!(
                "{name} is already being installed, upgraded or removed (from the Dev Tools page, an agent, `sawit pasang`, `sawit pupuk` or `sawit cabut`). Try again when that's done."
            )));
        }
        return Err(e.into());
    }
    Ok(file)
}

/// The tools that need `id` installed first.
pub fn dependents(id: &str) -> Vec<&'static Tool> {
    TOOLS.iter().filter(|t| t.needs.contains(&id)).collect()
}

/// Ends the process group `shell()` started (curl, bash and whatever the
/// script spawned) when the run is dropped before it exits.
struct KillGroup(Option<i32>);

impl Drop for KillGroup {
    fn drop(&mut self) {
        #[cfg(unix)]
        if let Some(pid) = self.0 {
            unsafe {
                libc::killpg(pid, libc::SIGTERM);
            }
        }
    }
}

async fn run_script(
    name: &str,
    command: &str,
    on_line: impl Fn(String) + Send + Sync,
) -> Result<()> {
    run_script_as(
        crate::probe::login_shell().as_deref(),
        name,
        command,
        on_line,
    )
    .await
}

/// [`run_script`] with `login` as `$SHELL`.
async fn run_script_as(
    login: Option<&str>,
    name: &str,
    command: &str,
    on_line: impl Fn(String) + Send + Sync,
) -> Result<()> {
    let script = format!("{PRELUDE}set -e\n{command}\n");
    let mut cmd = shell(login);
    cmd.arg("-c")
        .arg(script)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = cmd.spawn()?;
    // `setsid` made the shell a group leader, so its pid is the group's.
    let mut group = KillGroup(child.id().map(|pid| pid as i32));
    let (stdout, stderr) = (child.stdout.take(), child.stderr.take());
    let tail = std::sync::Mutex::new(Vec::<String>::new());
    let read = |pipe: Option<Box<dyn AsyncRead + Unpin + Send>>| {
        let (on_line, tail) = (&on_line, &tail);
        async move {
            let Some(pipe) = pipe else { return };
            let mut pipe = BufReader::new(pipe);
            let mut bytes = Vec::new();
            // Bytes, not `lines()`: a line that isn't UTF-8 would end the
            // reading there and close the pipe under the installer.
            loop {
                bytes.clear();
                match pipe.read_until(b'\n', &mut bytes).await {
                    Ok(0) | Err(_) => break,
                    Ok(_) => {}
                }
                let line = String::from_utf8_lossy(&bytes);
                let line = strip_ansi(line.trim_end_matches(['\n', '\r']));
                // Progress bars redraw with \r; keep what's left at the end.
                let line = line
                    .rsplit('\r')
                    .next()
                    .unwrap_or_default()
                    .trim_end()
                    .to_string();
                if line.trim().is_empty() {
                    continue;
                }
                {
                    let mut tail = tail.lock().unwrap();
                    tail.push(line.clone());
                    if tail.len() > 8 {
                        tail.remove(0);
                    }
                }
                on_line(line);
            }
        }
    };
    let wait = async {
        futures::join!(
            read(stdout.map(|p| Box::new(p) as Box<dyn AsyncRead + Unpin + Send>)),
            read(stderr.map(|p| Box::new(p) as Box<dyn AsyncRead + Unpin + Send>)),
        );
        child.wait().await
    };
    let status = tokio::time::timeout(Duration::from_secs(15 * 60), wait)
        .await
        .map_err(|_| CoreError::Command {
            program: name.into(),
            code: None,
            stderr: "timed out after 15 minutes".into(),
        })??;
    group.0 = None;
    if status.success() {
        Ok(())
    } else {
        Err(CoreError::Command {
            program: name.into(),
            code: status.code(),
            stderr: tail.into_inner().unwrap().join("\n"),
        })
    }
}

/// Drops terminal colour and cursor escapes (`ESC [ … letter`).
fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            if chars.peek() == Some(&'[') {
                chars.next();
                for c in chars.by_ref() {
                    if c.is_ascii_alphabetic() {
                        break;
                    }
                }
            } else {
                chars.next();
            }
        } else {
            out.push(c);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_unique_and_needs_exist() {
        for (ix, t) in TOOLS.iter().enumerate() {
            assert!(
                TOOLS[..ix].iter().all(|o| o.id != t.id),
                "duplicate {}",
                t.id
            );
            for need in t.needs {
                assert!(find(need).is_some(), "{} needs unknown {need}", t.id);
            }
        }
    }

    #[test]
    fn everything_but_npm_can_be_uninstalled() {
        for t in TOOLS {
            assert_eq!(t.uninstall.is_none(), t.id == "npm", "{}", t.id);
        }
        let ids: Vec<_> = dependents("node").iter().map(|t| t.id).collect();
        assert_eq!(ids, ["npm", "gemini", "cf"]);
    }

    /// PureMac is the app from its Homebrew cask, as its README installs it: Homebrew first,
    /// then `brew install --cask`, upgraded and removed with brew, whatever package manager is
    /// picked. It has no command of its own (`puremac` is the separate PureMac CLI), so
    /// detection looks for the app only.
    #[test]
    fn puremac_is_the_app_from_its_homebrew_cask() {
        let puremac = find("puremac").unwrap();
        assert_eq!(puremac.category, Category::Other);
        assert_eq!(puremac.cask, Some("puremac"));
        assert_eq!(puremac.latest, Latest::Cask("puremac"));
        assert_eq!(puremac.needs, ["homebrew"]);
        assert!(!puremac.interactive);
        for pm in [None, Some(PackageManager::Bun), Some(PackageManager::Pnpm)] {
            assert_eq!(install_command(puremac, pm), "brew install --cask puremac");
            assert_eq!(upgrade_command(puremac, pm), "brew upgrade --cask puremac");
        }
        assert_eq!(npm_package(puremac), None);
        assert_eq!(puremac.uninstall, Some("brew uninstall --cask puremac"));
        assert_eq!(
            puremac.also,
            ["/Applications/PureMac.app", "~/Applications/PureMac.app"]
        );
        let ids = |tools: Vec<&Tool>| tools.iter().map(|t| t.id).collect::<Vec<_>>();
        assert_eq!(ids(plan(&["puremac"], &|_| false)), ["homebrew", "puremac"]);
        assert_eq!(ids(plan(&["puremac"], &|id| id == "homebrew")), ["puremac"]);
        assert_eq!(ids(dependents("homebrew")), ["puremac"]);
        // No `command -v` for it: that would find the PureMac CLI.
        let script = detect_script();
        assert!(!script.contains(&format!("{MARK}puremac")), "{script}");
        assert!(!script.contains("command -v  "), "{script}");
        assert!(script.contains(&format!("{MARK}zed")), "{script}");
    }

    /// An app without a command is found where its cask (or a drag from the disk image) puts it:
    /// /Applications first, else ~/Applications.
    #[test]
    fn an_app_is_found_at_its_place() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path();
        let puremac = find("puremac").unwrap();
        let user_app = home.join("Applications/PureMac.app");
        std::fs::create_dir_all(user_app.join("Contents")).unwrap();
        // /Applications/PureMac.app isn't on this Mac when the test runs, unless it is.
        let expected = if Path::new("/Applications/PureMac.app").exists() {
            PathBuf::from("/Applications/PureMac.app")
        } else {
            user_app.clone()
        };
        assert_eq!(at_known_place(puremac, home), Some(expected));
        let other = tempfile::tempdir().unwrap();
        if !Path::new("/Applications/PureMac.app").exists() {
            assert_eq!(at_known_place(puremac, other.path()), None);
        }
    }

    /// An app's version is its Info.plist's CFBundleShortVersionString, in an XML or a binary
    /// plist; an app without one has none, and a tool that says its version is no use has none.
    #[tokio::test]
    async fn an_apps_version_comes_from_its_info_plist() {
        let dir = tempfile::tempdir().unwrap();
        let app = dir.path().join("PureMac.app");
        let contents = app.join("Contents");
        std::fs::create_dir_all(&contents).unwrap();
        let plist = contents.join("Info.plist");
        // As PureMac 3.2.0's Info.plist has them.
        std::fs::write(
            &plist,
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>CFBundleIdentifier</key>
	<string>com.puremac.app</string>
	<key>CFBundleShortVersionString</key>
	<string>3.2.0</string>
	<key>CFBundleVersion</key>
	<string>31</string>
</dict>
</plist>
"#,
        )
        .unwrap();
        let puremac = find("puremac").unwrap();
        assert_eq!(app_version(&app).await.as_deref(), Some("3.2.0"));
        assert_eq!(version_of(puremac, &app).await.as_deref(), Some("3.2.0"));
        // Cap says nothing useful about its version.
        assert_eq!(version_of(find("cap").unwrap(), &app).await, None);
        let binary = std::process::Command::new("/usr/bin/plutil")
            .args(["-convert", "binary1"])
            .arg(&plist)
            .status()
            .unwrap();
        assert!(binary.success());
        assert!(!std::fs::read(&plist).unwrap().starts_with(b"<?xml"));
        assert_eq!(app_version(&app).await.as_deref(), Some("3.2.0"));
        // No version in it, and no Info.plist at all.
        std::fs::write(
            &plist,
            r#"<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0"><dict><key>CFBundleVersion</key><string>31</string></dict></plist>
"#,
        )
        .unwrap();
        assert_eq!(app_version(&app).await, None);
        std::fs::remove_file(&plist).unwrap();
        assert_eq!(app_version(&app).await, None);
    }

    /// Homebrew's API names the version `brew upgrade --cask` installs.
    #[test]
    fn cask_versions() {
        assert_eq!(
            cask_version(r#"{"token":"puremac","version":"3.2.0","auto_updates":null}"#).as_deref(),
            Some("3.2.0")
        );
        assert_eq!(
            cask_version(r#"{"version":"1.2.3,456"}"#).as_deref(),
            Some("1.2.3")
        );
        assert_eq!(cask_version(r#"{"version":"latest"}"#), None);
        assert_eq!(cask_version(r#"{"token":"x"}"#), None);
        assert_eq!(cask_version("not json"), None);
        assert!(is_newer("3.3.0", "3.2.0"));
    }

    /// PureMac's copy is Homebrew's, and `brew uninstall --cask` runs on it, only where the cask
    /// puts the app and while Homebrew's Caskroom has the cask; one dragged in from the disk image
    /// is left alone, as is a copy anywhere else.
    #[test]
    fn a_cask_app_is_removed_only_when_homebrew_installed_it() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("me");
        let rooms = [dir.path().join("Caskroom")];
        let caskroom = &rooms[0];
        std::fs::create_dir_all(caskroom).unwrap();
        let puremac = find("puremac").unwrap();
        let script = |found: &Path| uninstall_command_at(puremac, found, found, &home, &rooms);
        let system = Path::new("/Applications/PureMac.app");
        let user = home.join("Applications/PureMac.app");
        let elsewhere = home.join("Downloads/PureMac.app");
        // Dragged in from the disk image: no Caskroom entry.
        assert_eq!(script(system), None);
        assert_eq!(script(&user), None);
        std::fs::create_dir_all(caskroom.join("puremac/3.2.0")).unwrap();
        let brew = Some("brew uninstall --cask puremac".to_string());
        assert_eq!(script(system), brew);
        assert_eq!(script(&user), brew);
        assert_eq!(script(&elsewhere), None);
        // Another cask's Caskroom folder isn't PureMac's.
        std::fs::remove_dir_all(caskroom.join("puremac")).unwrap();
        std::fs::create_dir_all(caskroom.join("pesty")).unwrap();
        assert_eq!(script(system), None);
        // As the page and `sawit cabut` work it out: left alone, or run.
        let found = HashMap::from([(
            "puremac",
            Found {
                path: system.to_path_buf(),
                version: Some("3.2.0".into()),
                outside: None,
            },
        )]);
        let plan = || {
            uninstall_plan_with(&["puremac"], &found, &|tool, path| {
                uninstall_command_at(tool, path, path, &home, &rooms)
            })
        };
        assert_eq!(plan()[0].removal, Removal::LeftAlone);
        std::fs::create_dir_all(caskroom.join("puremac")).unwrap();
        assert_eq!(
            plan()[0].removal,
            Removal::Run("brew uninstall --cask puremac".into())
        );
        // A cask's Caskroom doesn't make another app Homebrew's: Zed's app, with no command of
        // its install script leading into it, is left alone.
        assert_eq!(
            uninstall_command_at(
                find("zed").unwrap(),
                Path::new("/Applications/Zed.app"),
                Path::new("/Applications/Zed.app"),
                &home,
                &rooms
            ),
            None
        );
    }

    /// Every app of the catalog (an app bundle among its places) is one whose copy the rule of
    /// `installed_outside` works out: Cap, Zed and PureMac, each with an uninstaller, which
    /// that rule is built on.
    #[test]
    fn the_catalogs_apps_are_cap_zed_and_puremac() {
        let apps: Vec<&str> = TOOLS
            .iter()
            .filter(|t| is_app_tool(t))
            .map(|t| t.id)
            .collect();
        assert_eq!(apps, ["cap", "zed", "puremac"]);
        for id in apps {
            assert!(find(id).unwrap().uninstall.is_some(), "{id}");
        }
    }

    /// An app found at its place is its install script's copy only while the command the script
    /// links leads into it (Zed's ~/.local/bin/zed, Cap's ~/.cap/bin/cap): then Sawit upgrades
    /// and removes it. Without one it was dragged in from the disk image and keeps itself up to
    /// date: "Installed outside its install script", no Upgrade and no Uninstall. A cask's app
    /// without its Caskroom entry is "Installed outside Homebrew". Other tools are as before.
    #[test]
    fn an_app_is_its_scripts_only_with_its_command_linked() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("me");
        let rooms = [dir.path().join("Caskroom")];
        std::fs::create_dir_all(&rooms[0]).unwrap();
        let app = |name: &str, program: &str| {
            let app = home.join(format!("Applications/{name}.app"));
            let macos = app.join("Contents/MacOS");
            std::fs::create_dir_all(&macos).unwrap();
            std::fs::write(macos.join(program), "").unwrap();
            app
        };
        let link = |from: &str, to: &Path| {
            let from = home.join(from);
            std::fs::create_dir_all(from.parent().unwrap()).unwrap();
            let _ = std::fs::remove_file(&from);
            std::os::unix::fs::symlink(to, from).unwrap();
        };
        let (uninstall, outside) = (
            |id: &str, found: &Path| {
                let real = std::fs::canonicalize(found).unwrap();
                uninstall_command_at(find(id).unwrap(), found, &real, &home, &rooms)
            },
            |id: &str, found: &Path| {
                let real = std::fs::canonicalize(found).unwrap();
                installed_outside_at(find(id).unwrap(), found, &real, &home, &rooms)
            },
        );
        let own = |id: &str| find(id).unwrap().uninstall.map(String::from);

        let zed = app("Zed", "cli");
        assert_eq!(uninstall("zed", &zed), None);
        assert_eq!(outside("zed", &zed), Some(Outside::InstallScript));
        // A link into another Zed doesn't make this one the script's.
        let other = dir.path().join("elsewhere/Zed.app/Contents/MacOS");
        std::fs::create_dir_all(&other).unwrap();
        std::fs::write(other.join("cli"), "").unwrap();
        link(".local/bin/zed", &other.join("cli"));
        assert_eq!(outside("zed", &zed), Some(Outside::InstallScript));
        link(".local/bin/zed", &zed.join("Contents/MacOS/cli"));
        assert_eq!(uninstall("zed", &zed), own("zed"));
        assert_eq!(outside("zed", &zed), None);
        // Found by that command, as detection usually finds it: the script's.
        assert_eq!(outside("zed", &home.join(".local/bin/zed")), None);

        // Cap's shim in ~/.cap/bin, which the login shell may not have on its PATH.
        let cap = app("Cap", "cap-cli");
        assert_eq!(outside("cap", &cap), Some(Outside::InstallScript));
        link(".cap/bin/cap", &cap.join("Contents/MacOS/cap-cli"));
        assert_eq!(uninstall("cap", &cap), own("cap"));
        assert_eq!(outside("cap", &cap), None);

        let puremac = app("PureMac", "PureMac");
        assert_eq!(outside("puremac", &puremac), Some(Outside::Homebrew));
        std::fs::create_dir_all(rooms[0].join("puremac/3.2.0")).unwrap();
        assert_eq!(outside("puremac", &puremac), None);

        // Not an app: whatever copy it is, its upgrade is offered as before.
        let bun = home.join("bin/bun");
        std::fs::create_dir_all(bun.parent().unwrap()).unwrap();
        std::fs::write(&bun, "").unwrap();
        assert_eq!(uninstall("bun", &bun), None);
        assert_eq!(outside("bun", &bun), None);
    }

    /// An app installed outside what upgrades it shows its version, but never an upgrade: not in
    /// `newer` (the page's Update badge and Upgrade All, list_dev_tools, `sawit list`), and not
    /// in `upgrade_plan`, whether every tool is upgraded or it's named (upgrade_dev_tools,
    /// `sawit pupuk puremac`), even when its latest release is unknown.
    #[test]
    fn an_app_installed_outside_is_never_upgraded() {
        let at = |path: &str, v: &str, outside: Option<Outside>| Found {
            path: PathBuf::from(path),
            version: Some(v.into()),
            outside,
        };
        let dmg = Some(Outside::Homebrew);
        let script = Some(Outside::InstallScript);
        let mut found = HashMap::from([
            ("puremac", at("/Applications/PureMac.app", "3.1.0", dmg)),
            ("zed", at("/Applications/Zed.app", "0.200.0", script)),
            ("cap", at("/Applications/Cap.app", "1.0.0", script)),
            ("bun", at("/Users/me/.bun/bin/bun", "1.0.0", None)),
        ]);
        let latest = HashMap::from([
            ("puremac", "3.2.0".to_string()),
            ("zed", "0.210.0".to_string()),
            ("bun", "1.1.0".to_string()),
        ]);
        let ids = |p: Vec<&Tool>| p.iter().map(|t| t.id).collect::<Vec<_>>();
        assert_eq!(super::newer("puremac", &found, &latest), None);
        assert_eq!(super::newer("zed", &found, &latest), None);
        assert_eq!(ids(upgrade_plan(&[], &found, &latest)), ["bun"]);
        assert_eq!(
            ids(upgrade_plan(
                &["puremac", "zed", "cap", "bun"],
                &found,
                &latest
            )),
            ["bun"]
        );
        // Homebrew's copy, and Zed's script's: upgraded.
        found.get_mut("puremac").unwrap().outside = None;
        found.get_mut("zed").unwrap().outside = None;
        assert_eq!(super::newer("puremac", &found, &latest), Some("3.2.0"));
        assert_eq!(
            ids(upgrade_plan(&[], &found, &latest)),
            ["bun", "zed", "puremac"]
        );
        // Cap publishes no latest release: named, it runs its script again, as before.
        found.get_mut("cap").unwrap().outside = None;
        assert_eq!(ids(upgrade_plan(&["cap"], &found, &latest)), ["cap"]);
    }

    /// Removing Homebrew warns per kind of tool that needs it: an app from its cask keeps
    /// working but won't get updates through Homebrew any more; a tool that runs through what's
    /// removed stops working. A copy installed outside it (PureMac from its disk image) isn't
    /// named at all.
    #[test]
    fn removing_homebrew_warns_per_kind_of_dependent() {
        let tool = |id: &str| find(id).unwrap();
        assert_eq!(impact(tool("puremac")), Impact::NoHomebrewUpdates);
        assert_eq!(impact(tool("gemini")), Impact::StopsWorking);
        assert_eq!(
            needed_by_warning(&[tool("puremac")]),
            "PureMac won't get updates through Homebrew any more"
        );
        assert_eq!(
            needed_by_warning(&[tool("npm"), tool("gemini")]),
            "npm (latest), Gemini CLI need it and will stop working"
        );
        assert_eq!(
            needed_by_warning(&[tool("node")]),
            "Node.js needs it and will stop working"
        );
        assert_eq!(
            needed_by_warning(&[tool("puremac"), tool("gemini")]),
            "Gemini CLI needs it and will stop working; PureMac won't get updates through Homebrew any more"
        );
        assert_eq!(needed_by_warning(&[]), "");

        let at = |path: &str, outside: Option<Outside>| Found {
            path: PathBuf::from(path),
            version: None,
            outside,
        };
        let mut found = HashMap::from([
            ("homebrew", at("/opt/homebrew/bin/brew", None)),
            ("puremac", at("/Applications/PureMac.app", None)),
        ]);
        let command = |_: &Tool, _: &Path| Some("uninstall.sh".to_string());
        let plan = uninstall_plan_with(&["homebrew"], &found, &command);
        assert_eq!(
            plan[0].needed_by.iter().map(|t| t.id).collect::<Vec<_>>(),
            ["puremac"]
        );
        assert_eq!(
            needed_by_warning(&plan[0].needed_by),
            "PureMac won't get updates through Homebrew any more"
        );
        found.get_mut("puremac").unwrap().outside = Some(Outside::Homebrew);
        let plan = uninstall_plan_with(&["homebrew"], &found, &command);
        assert!(plan[0].needed_by.is_empty());
    }

    /// The tools npm installs globally install with the package manager picked, the way the cf
    /// docs list them; npm's own update and the curl installers stay as they are.
    #[test]
    fn npm_tools_install_with_the_package_manager_picked() {
        let tool = |id: &str| find(id).unwrap();
        let npm_tools: Vec<&str> = TOOLS
            .iter()
            .filter(|t| npm_package(t).is_some())
            .map(|t| t.id)
            .collect();
        assert_eq!(npm_tools, ["gemini", "cf"]);
        assert_eq!(
            install_command(tool("cf"), Some(PackageManager::Bun)),
            "bun add -g cf"
        );
        assert_eq!(
            install_command(tool("gemini"), Some(PackageManager::Pnpm)),
            "pnpm add -g @google/gemini-cli"
        );
        assert_eq!(
            upgrade_command(tool("gemini"), Some(PackageManager::Yarn)),
            "yarn global add @google/gemini-cli"
        );
        assert_eq!(install_command(tool("cf"), None), "npm install -g cf");
        assert_eq!(
            install_command(tool("npm"), Some(PackageManager::Bun)),
            "npm install -g npm@latest"
        );
        assert_eq!(
            install_command(tool("bun"), Some(PackageManager::Pnpm)),
            tool("bun").install
        );
        assert_eq!(
            upgrade_command(tool("bun"), Some(PackageManager::Pnpm)),
            "bun upgrade"
        );
    }

    #[test]
    fn uninstall_follows_who_installed_it() {
        let home = std::path::Path::new("/Users/me");
        let p = std::path::Path::new;
        let uv = find("uv").unwrap();
        // Homebrew's copy: brew removes it, not uv's own script.
        assert_eq!(
            uninstall_command_in(
                uv,
                p("/opt/homebrew/bin/uv"),
                p("/opt/homebrew/Cellar/uv/0.12.1/bin/uv"),
                home
            )
            .as_deref(),
            Some("brew uninstall uv")
        );
        // The installer's copy under the home folder.
        assert_eq!(
            uninstall_command_in(
                uv,
                p("/Users/me/.local/bin/uv"),
                p("/Users/me/.local/bin/uv"),
                home
            ),
            uv.uninstall.map(String::from)
        );
        // Somewhere else: left alone.
        assert_eq!(
            uninstall_command_in(uv, p("/usr/local/bin/uv"), p("/usr/local/bin/uv"), home),
            None
        );
        // Homebrew itself, at its known location.
        let brew = find("homebrew").unwrap();
        assert!(
            uninstall_command_in(
                brew,
                p("/opt/homebrew/bin/brew"),
                p("/opt/homebrew/bin/brew"),
                home
            )
            .is_some_and(|c| c.contains("uninstall.sh"))
        );
    }

    /// "Under the home folder" isn't "put there by this tool's installer":
    /// another manager's copy gets no removal script (or its own manager's),
    /// never the catalog's, which would remove something else.
    #[test]
    fn uninstall_is_offered_only_for_the_copy_the_installer_made() {
        let home = std::path::Path::new("/Users/me");
        let p = std::path::Path::new;
        let script = |id: &str, found: &str, real: &str| {
            uninstall_command_in(find(id).unwrap(), p(found), p(real), home)
        };
        let own = |id: &str| find(id).unwrap().uninstall.map(String::from);
        // pyenv's and asdf's shims: `uv python uninstall --all` would remove
        // every uv Python and leave theirs.
        for shim in [
            "/Users/me/.pyenv/shims/python3",
            "/Users/me/.asdf/shims/python3",
        ] {
            assert_eq!(script("python", shim, shim), None, "{shim}");
        }
        assert_eq!(
            script(
                "python",
                "/Users/me/.local/bin/python3",
                "/Users/me/.local/share/uv/python/cpython-3.13.2-macos-aarch64-none/bin/python3.13"
            ),
            own("python")
        );
        // Claude Code from npm: npm removes it, not the native installer's script.
        let npm = "/Users/me/.nvm/versions/node/v22.11.0/lib/node_modules/@anthropic-ai/claude-code/cli.js";
        assert_eq!(
            script(
                "claude-code",
                "/Users/me/.nvm/versions/node/v22.11.0/bin/claude",
                npm
            )
            .as_deref(),
            Some("npm uninstall -g @anthropic-ai/claude-code")
        );
        // Installed with Bun or pnpm: that one removes it.
        assert_eq!(
            script(
                "gemini",
                "/Users/me/.bun/bin/gemini",
                "/Users/me/.bun/install/global/node_modules/@google/gemini-cli/dist/index.js"
            )
            .as_deref(),
            Some("bun remove -g @google/gemini-cli")
        );
        assert_eq!(
            script(
                "cf",
                "/Users/me/Library/pnpm/cf",
                "/Users/me/Library/pnpm/global/5/node_modules/cf/bin/cf"
            )
            .as_deref(),
            Some("pnpm remove -g cf")
        );
        assert_eq!(
            script(
                "claude-code",
                "/Users/me/.local/bin/claude",
                "/Users/me/.local/share/claude/versions/2.0.14"
            ),
            own("claude-code")
        );
        // Capistrano's `cap` isn't Cap's; Cap's own is a link into Cap.app.
        let rbenv = "/Users/me/.rbenv/shims/cap";
        assert_eq!(script("cap", rbenv, rbenv), None);
        assert_eq!(
            script(
                "cap",
                "/Users/me/.local/bin/cap",
                "/Applications/Cap.app/Contents/MacOS/cap-cli"
            ),
            own("cap")
        );
        // Bun from npm, and omp from bun: not where their installers put them.
        assert_eq!(
            script(
                "bun",
                "/Users/me/.nvm/versions/node/v22.11.0/bin/bun",
                "/Users/me/.nvm/versions/node/v22.11.0/lib/node_modules/bun/bin/bun.exe"
            ),
            None
        );
        assert_eq!(
            script("omp", "/Users/me/.bun/bin/omp", "/Users/me/.bun/bin/omp"),
            None
        );
        assert_eq!(
            script("bun", "/Users/me/.bun/bin/bun", "/Users/me/.bun/bin/bun"),
            own("bun")
        );
        // Zed's CLI from its installer, and from Homebrew's cask, which links the same app.
        let zed_cli = "/Applications/Zed.app/Contents/MacOS/cli";
        assert_eq!(
            script("zed", "/Users/me/.local/bin/zed", zed_cli),
            own("zed")
        );
        assert_eq!(script("zed", "/opt/homebrew/bin/zed", zed_cli), None);
        // The app found on its own, with no ~/.local/bin/zed leading into it: dragged in from
        // the disk image, not the script's (an_app_is_its_scripts_only_with_its_command_linked).
        assert_eq!(
            script("zed", "/Applications/Zed.app", "/Applications/Zed.app"),
            None
        );
        // A Cellar folder whose name isn't a formula's isn't put in a command.
        assert_eq!(
            script(
                "uv",
                "/opt/homebrew/bin/uv",
                "/opt/homebrew/Cellar/uv;touch${IFS}x/0.1/bin/uv"
            ),
            None
        );
    }

    /// npm comes with Node.js and has no uninstaller of its own. Homebrew's
    /// npm is a file of its `node` formula, so `brew uninstall node` would
    /// take Node.js with it: npm gets no command, wherever it was found.
    #[test]
    fn npm_is_never_uninstalled_on_its_own() {
        let home = std::path::Path::new("/Users/me");
        let p = std::path::Path::new;
        let npm = find("npm").unwrap();
        assert_eq!(
            uninstall_command_in(
                npm,
                p("/opt/homebrew/bin/npm"),
                p("/opt/homebrew/Cellar/node/24.1.0/lib/node_modules/npm/bin/npm-cli.js"),
                home
            ),
            None
        );
        let nvm = "/Users/me/.nvm/versions/node/v22.11.0/bin/npm";
        assert_eq!(uninstall_command_in(npm, p(nvm), p(nvm), home), None);
    }

    /// `uninstall_dev_tools` and `sawit cabut` run only what the page's trash
    /// button would: the command for the copy the tool's installer, Homebrew
    /// or npm put in place, never one for a copy installed another way.
    /// Homebrew's own uninstaller goes to a terminal, a tool goes before the
    /// tools it needs, and the installed tools left behind that need one are
    /// named.
    #[test]
    fn an_uninstall_runs_only_what_the_page_would_and_in_order() {
        let home = std::path::Path::new("/Users/me");
        let gemini = "/Users/me/.nvm/versions/node/v22.11.0/bin/gemini";
        // What a found path leads to, as canonicalize would say.
        let real = |path: &Path| match path.to_str() {
            Some(p) if p == gemini => PathBuf::from(
                "/Users/me/.nvm/versions/node/v22.11.0/lib/node_modules/@google/gemini-cli/dist/index.js",
            ),
            _ => path.to_path_buf(),
        };
        let command =
            |tool: &Tool, path: &Path| uninstall_command_in(tool, path, &real(path), home);
        let at = |path: &str| Found {
            path: PathBuf::from(path),
            version: None,
            outside: None,
        };
        let found: HashMap<&'static str, Found> = HashMap::from([
            ("homebrew", at("/opt/homebrew/bin/brew")),
            ("nvm", at("/Users/me/.nvm/nvm.sh")),
            ("node", at("/Users/me/.nvm/versions/node/v22.11.0/bin/node")),
            ("npm", at("/Users/me/.nvm/versions/node/v22.11.0/bin/npm")),
            ("gemini", at(gemini)),
            ("uv", at("/Users/me/.local/bin/uv")),
            ("python", at("/Users/me/.local/bin/python3")),
            // Capistrano's, from rbenv: not Cap's.
            ("cap", at("/Users/me/.rbenv/shims/cap")),
            ("bun", at("/Users/me/.bun/bin/bun")),
        ]);
        let own = |id: &str| find(id).unwrap().uninstall.unwrap().to_string();
        let ids = |tools: &[&'static Tool]| tools.iter().map(|t| t.id).collect::<Vec<_>>();

        let wanted = [
            "uv", "cap", "homebrew", "nvm", "gemini", "npm", "bun", "bun", "nope", "deno",
        ];
        let plan = uninstall_plan_with(&wanted, &found, &command);
        let got: Vec<(&str, Removal)> = plan
            .iter()
            .map(|u| (u.tool.id, u.removal.clone()))
            .collect();
        assert_eq!(
            got,
            vec![
                // They need Node.js, which nvm installs: before both.
                (
                    "gemini",
                    Removal::Run("npm uninstall -g @google/gemini-cli".into())
                ),
                ("npm", Removal::Bundled(vec!["Node.js"])),
                ("uv", Removal::Run(own("uv"))),
                ("cap", Removal::LeftAlone),
                ("homebrew", Removal::Terminal(own("homebrew"))),
                ("nvm", Removal::Run(own("nvm"))),
                ("bun", Removal::Run(own("bun"))),
                ("deno", Removal::Missing),
            ]
        );
        assert_eq!(
            plan[3].found,
            Some(PathBuf::from("/Users/me/.rbenv/shims/cap"))
        );
        assert_eq!(plan[7].found, None);
        // uv's Python and nvm's Node.js stay, and stop working.
        let needed: Vec<(&str, Vec<&str>)> = plan
            .iter()
            .map(|u| (u.tool.id, ids(&u.needed_by)))
            .collect();
        assert_eq!(
            needed,
            vec![
                ("gemini", vec![]),
                ("npm", vec![]),
                ("uv", vec!["python"]),
                ("cap", vec![]),
                ("homebrew", vec![]),
                ("nvm", vec!["node"]),
                ("bun", vec![]),
                ("deno", vec![]),
            ]
        );

        // Removed together, nothing is left needing them but npm, which comes with Node.js.
        let plan = uninstall_plan_with(&["nvm", "node", "gemini"], &found, &command);
        let order: Vec<&str> = plan.iter().map(|u| u.tool.id).collect();
        assert_eq!(order, ["gemini", "node", "nvm"]);
        assert_eq!(ids(&plan[1].needed_by), ["npm"]);
        assert!(plan[0].needed_by.is_empty() && plan[2].needed_by.is_empty());
    }

    #[tokio::test]
    async fn dropping_a_run_stops_everything_it_started() {
        let marker = format!("sleep 31.{}", std::process::id());
        let script = format!("{marker} & {marker}");
        let alive = || {
            std::process::Command::new("/usr/bin/pgrep")
                .args(["-f", &marker])
                .output()
                .is_ok_and(|o| o.status.success())
        };
        let run = run_script("test", &script, |_| {});
        assert!(
            tokio::time::timeout(Duration::from_millis(1500), run)
                .await
                .is_err()
        );
        tokio::time::sleep(Duration::from_millis(300)).await;
        assert!(!alive(), "the script's processes outlived the run");
    }

    /// fish, nu, PowerShell and tcsh can't run the scripts: they run in zsh.
    /// tcsh ships with macOS, so it stands in for them here.
    #[tokio::test]
    async fn scripts_run_whatever_the_login_shell() {
        let lines = std::sync::Mutex::new(Vec::new());
        run_script_as(Some("/bin/tcsh"), "test", "echo \"${NVM_DIR:+nvm}\"", |l| {
            lines.lock().unwrap().push(l)
        })
        .await
        .unwrap();
        assert_eq!(*lines.lock().unwrap(), ["nvm"]);
        let found = detect_as(Some("/bin/tcsh")).await;
        assert_eq!(found.problem, None);
    }

    /// A login shell that fails is reported, not read as "nothing installed".
    #[tokio::test]
    async fn a_failing_login_shell_is_reported() {
        use std::os::unix::fs::PermissionsExt as _;
        let dir = tempfile::tempdir().unwrap();
        // Named `sh`, so it's taken for a POSIX shell and run.
        let sh = dir.path().join("sh");
        std::fs::write(&sh, "#!/bin/sh\necho 'broken profile' >&2\nexit 3\n").unwrap();
        std::fs::set_permissions(&sh, std::fs::Permissions::from_mode(0o755)).unwrap();
        let found = detect_as(sh.to_str()).await;
        let problem = found.problem.expect("no problem reported");
        assert!(problem.contains("broken profile"), "{problem}");
        assert!(problem.contains("exit 3"), "{problem}");
    }

    /// An installer that prints a line in Latin-1 goes on being read: the
    /// rest of its output still shows, and its pipe isn't closed under it.
    #[tokio::test]
    async fn output_after_a_line_that_isnt_utf8_still_comes_through() {
        let lines = std::sync::Mutex::new(Vec::new());
        run_script("test", r"printf 'caf\351\n'; echo after", |l| {
            lines.lock().unwrap().push(l)
        })
        .await
        .unwrap();
        assert_eq!(*lines.lock().unwrap(), ["caf\u{FFFD}", "after"]);
    }

    /// The MCP page looks programs up where the Dev Tools page finds tools,
    /// not only on the app's `PATH`, which misses what `~/.zshrc` adds.
    #[test]
    fn programs_are_found_where_installers_put_them() {
        use std::os::unix::fs::PermissionsExt as _;
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path();
        let program = |path: &Path, text: &str| {
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, text).unwrap();
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
        };
        program(&home.join(".local/bin/claude"), "");
        let nvm = home.join(".nvm");
        for v in ["v18.20.4", "v22.11.0", "v23.1.0"] {
            program(&nvm.join(format!("versions/node/{v}/bin/npx")), "");
        }
        std::fs::create_dir_all(nvm.join("alias/lts")).unwrap();
        std::fs::write(nvm.join("alias/default"), "lts/*\n").unwrap();
        std::fs::write(nvm.join("alias/lts/*"), "lts/jod\n").unwrap();
        std::fs::write(nvm.join("alias/lts/jod"), "v22.11.0\n").unwrap();
        // Capistrano's `cap`, a Ruby script: not Cap's.
        program(&home.join(".local/bin/cap"), "#!/usr/bin/env ruby\n");
        let empty = std::ffi::OsString::new();
        let find = |name| find_program_in(name, Some(&empty), home, &nvm, &[]);
        assert_eq!(find("claude"), Some(home.join(".local/bin/claude")));
        assert_eq!(
            find("npx"),
            Some(nvm.join("versions/node/v22.11.0/bin/npx"))
        );
        assert_eq!(find("cap"), None);
        assert_eq!(find("codex"), None);
        // The default named by number, and an nvm with no default: the newest.
        std::fs::write(nvm.join("alias/default"), "18").unwrap();
        assert_eq!(
            find("npx"),
            Some(nvm.join("versions/node/v18.20.4/bin/npx"))
        );
        std::fs::remove_file(nvm.join("alias/default")).unwrap();
        assert_eq!(find("npx"), Some(nvm.join("versions/node/v23.1.0/bin/npx")));
        // PATH comes first.
        program(&home.join("bin/claude"), "");
        let path = std::env::join_paths([home.join("bin")]).unwrap();
        assert_eq!(
            find_program_in("claude", Some(&path), home, &nvm, &[]),
            Some(home.join("bin/claude"))
        );
        // Every folder searched is one detection searches too.
        for d in INSTALL_DIRS.iter().chain(&SYSTEM_DIRS) {
            let d = d.replace("~/", "$HOME/");
            assert!(PRELUDE.contains(&format!("{d}:")), "{d}");
        }
    }

    /// Two runs for one tool (the page, an agent, `sawit pasang`) never
    /// overlap: the second says the first is running, and names every place
    /// a run can come from.
    #[tokio::test]
    async fn one_run_per_tool_at_a_time() {
        let id = format!("test-{}", std::process::id());
        let first = run_locked(&id, "Test", "sleep 2", |_| {});
        let second = async {
            tokio::time::sleep(Duration::from_millis(500)).await;
            run_locked(&id, "Test", "true", |_| {}).await
        };
        let (first, second) = tokio::join!(first, second);
        first.unwrap();
        let err = second.expect_err("ran alongside the first");
        assert!(err.to_string().contains("already"), "{err}");
        for word in ["sawit pasang", "sawit pupuk", "sawit cabut"] {
            assert!(err.to_string().contains(word), "{err}");
        }
        // Once the first is done, the next runs. A process another test forked while the lock
        // was held has the lock's file too until it execs, and so holds the lock until then;
        // under load that can outlast the first run by a moment. So the next run may say
        // "already" for a few seconds, but no longer.
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        loop {
            match run_locked(&id, "Test", "true", |_| {}).await {
                Ok(()) => break,
                Err(e)
                    if e.to_string().contains("already")
                        && std::time::Instant::now() < deadline =>
                {
                    tokio::time::sleep(Duration::from_millis(100)).await;
                }
                Err(e) => panic!("the next run didn't run once the first was done: {e}"),
            }
        }
    }

    #[test]
    fn plan_puts_missing_prerequisites_first() {
        let none = |_: &str| false;
        let ids: Vec<_> = plan(&["gemini", "npm", "python"], &none)
            .iter()
            .map(|t| t.id)
            .collect();
        assert_eq!(ids, ["nvm", "node", "gemini", "npm", "uv", "python"]);
    }

    #[test]
    fn plan_skips_installed_prerequisites() {
        let installed = |id: &str| id == "nvm" || id == "uv";
        let ids: Vec<_> = plan(&["gemini", "python"], &installed)
            .iter()
            .map(|t| t.id)
            .collect();
        assert_eq!(ids, ["node", "gemini", "python"]);
    }

    #[test]
    fn versions() {
        assert_eq!(short_version("Python 3.13.2\n").as_deref(), Some("3.13.2"));
        assert_eq!(short_version("v22.11.0").as_deref(), Some("22.11.0"));
        assert_eq!(
            short_version("2.0.14 (Claude Code)").as_deref(),
            Some("2.0.14")
        );
        assert_eq!(
            short_version("rustc 1.90.0 (1159e78c4 2025-09-14)").as_deref(),
            Some("1.90.0")
        );
        assert_eq!(
            short_version("uv 0.12.1 (Homebrew)").as_deref(),
            Some("0.12.1")
        );
        assert_eq!(short_version("nothing here"), None);
    }

    #[test]
    fn tags() {
        assert_eq!(tag_version("v2.9.8").as_deref(), Some("2.9.8"));
        assert_eq!(tag_version("bun-v1.4.4").as_deref(), Some("1.4.4"));
        assert_eq!(tag_version("7.0.11").as_deref(), Some("7.0.11"));
        assert_eq!(tag_version("nightly"), None);
    }

    #[test]
    fn upgrades() {
        let at = |v: &str| Found {
            path: PathBuf::from("/x"),
            version: Some(v.into()),
            outside: None,
        };
        // bun is behind, uv is current, deno publishes nothing readable.
        let found = HashMap::from([
            ("bun", at("1.0.0")),
            ("uv", at("0.5.0")),
            ("deno", at("2.0")),
        ]);
        let latest = HashMap::from([("bun", "1.1.0".to_string()), ("uv", "0.5.0".to_string())]);
        assert_eq!(super::newer("bun", &found, &latest), Some("1.1.0"));
        assert_eq!(super::newer("uv", &found, &latest), None);
        assert_eq!(super::newer("pnpm", &found, &latest), None);
        let ids = |p: Vec<&Tool>| p.iter().map(|t| t.id).collect::<Vec<_>>();
        assert_eq!(ids(upgrade_plan(&[], &found, &latest)), ["bun"]);
        // Named: current ones are skipped, unknown latest still runs, missing ones never.
        assert_eq!(
            ids(upgrade_plan(
                &["uv", "deno", "pnpm", "bun"],
                &found,
                &latest
            )),
            ["bun", "deno"]
        );
    }

    #[test]
    fn newer() {
        assert!(is_newer("1.3.0", "1.2.9"));
        assert!(is_newer("1.10.0", "1.9.0"));
        assert!(is_newer("2.0", "1.99.1"));
        assert!(is_newer("1.2.3.1", "1.2.3"));
        assert!(!is_newer("1.2.3", "1.2.3"));
        assert!(!is_newer("1.2", "1.2.0"));
        assert!(!is_newer("1.2.3", "1.3.0"));
        assert!(!is_newer("1.3.0-beta.1", "1.3.0"));
        assert!(is_newer("v1.3.0-rc.1", "1.2.0"));
        assert!(!is_newer(
            "0.0.1793260836-gdb4c2d",
            "0.0.1793260836-gdb4c2d"
        ));
        assert!(!is_newer("nightly", "1.0.0"));
    }

    /// Hits the network: `cargo test -p sawit-core dev_tools -- --ignored`.
    #[tokio::test]
    #[ignore]
    async fn latest_lookups() {
        let latest = latest_versions().await;
        for id in ["bun", "deno", "pnpm", "claude-code", "puremac"] {
            let v = latest
                .get(id)
                .unwrap_or_else(|| panic!("no latest for {id}"));
            assert!(v.starts_with(|c: char| c.is_ascii_digit()), "{id}: {v}");
        }
    }

    #[test]
    fn ansi_is_stripped() {
        assert_eq!(strip_ansi("\u{1b}[1;32mok\u{1b}[0m done"), "ok done");
    }
}
