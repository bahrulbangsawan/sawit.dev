//! Developer tools that install with a one-line cURL script (Claude Code,
//! Bun, uv, …): the catalog, which of them are already installed, the order
//! to install a selection in (prerequisites first), the latest released
//! versions, and running an installer or upgrade while streaming its output.

use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;

use tokio::io::{AsyncBufReadExt, AsyncRead, BufReader};

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
    /// The command that shows it is installed.
    pub bin: &'static str,
    /// Where it lands when the installer doesn't put it on `PATH` (`~` is
    /// `$HOME`).
    pub also: &'static [&'static str],
    /// The install command, run by the login shell.
    pub install: &'static str,
    /// The command that removes what `install` put in place, when there is
    /// one (npm ships with Node.js, so it has none).
    pub uninstall: Option<&'static str>,
    /// Ids of tools that must be installed first.
    pub needs: &'static [&'static str],
    /// Asks for the user's password, so it runs in the terminal instead.
    pub interactive: bool,
    /// `bin --version` prints something useful (and returns quickly).
    pub version: bool,
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
        install,
        uninstall: None,
        needs: &[],
        interactive: false,
        version: true,
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
        uninstall: Some(
            r#"rm -f "$(command -v cap)"
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
];

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

/// Put in front of every command: the folders installers use, and nvm, so a
/// tool installed a moment ago is found even before the shell profile
/// picks it up.
const PRELUDE: &str = r#"export PATH="$HOME/.local/bin:$HOME/.cargo/bin:$HOME/.bun/bin:$HOME/.deno/bin:$HOME/.fly/bin:$HOME/.opencode/bin:$HOME/.amp/bin:$HOME/Library/pnpm:/opt/homebrew/bin:/usr/local/bin:$PATH"
export NVM_DIR="${NVM_DIR:-$HOME/.nvm}"
[ -s "$NVM_DIR/nvm.sh" ] && . "$NVM_DIR/nvm.sh" >/dev/null 2>&1
"#;

/// The user's login shell, so the profile's `PATH` and variables apply.
fn shell() -> tokio::process::Command {
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".into());
    let mut cmd = tokio::process::Command::new(shell);
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

fn expand(path: &str) -> PathBuf {
    match path.strip_prefix("~/") {
        Some(rest) => PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(rest),
        None => PathBuf::from(path),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    pub path: PathBuf,
    pub version: Option<String>,
}

const MARK: &str = "__SAWIT_TOOL__";

/// Which tools are installed: `command -v` in one login shell, then the
/// known install locations, then each one's `--version`.
pub async fn detect() -> HashMap<&'static str, Found> {
    let mut script = String::from(PRELUDE);
    for t in TOOLS {
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
    let mut cmd = shell();
    cmd.arg("-c")
        .arg(script)
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let out = match tokio::time::timeout(Duration::from_secs(20), cmd.output()).await {
        Ok(Ok(out)) => String::from_utf8_lossy(&out.stdout).into_owned(),
        _ => String::new(),
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
        {
            paths.insert(t.id, PathBuf::from(path));
        }
    }
    for t in TOOLS {
        if paths.contains_key(t.id) {
            continue;
        }
        if let Some(p) = t.also.iter().map(|p| expand(p)).find(|p| p.exists()) {
            paths.insert(t.id, p);
        }
    }
    let versions = futures::future::join_all(paths.iter().map(|(id, path)| async move {
        let tool = find(id)?;
        if !tool.version || path.extension().is_some_and(|e| e == "app") {
            return None;
        }
        let out = crate::cmd::run(path, ["--version"], Duration::from_secs(3))
            .await
            .ok()?;
        let text = if out.stdout.trim().is_empty() {
            out.stderr
        } else {
            out.stdout
        };
        Some((*id, short_version(&text)?))
    }))
    .await;
    let mut versions: HashMap<&str, String> = versions.into_iter().flatten().collect();
    paths
        .into_iter()
        .map(|(id, path)| {
            let version = versions.remove(id);
            (id, Found { path, version })
        })
        .collect()
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

/// The newer release installed tool `id` can upgrade to.
pub fn newer<'a>(
    id: &str,
    found: &HashMap<&'static str, Found>,
    latest: &'a HashMap<&'static str, String>,
) -> Option<&'a str> {
    let installed = found.get(id)?.version.as_ref()?;
    let latest = latest.get(id)?;
    is_newer(latest, installed).then_some(latest.as_str())
}

/// The tools upgrading `wanted` runs, in catalog order: every installed
/// tool with a newer release when `wanted` is empty, else each wanted tool
/// that is installed and not known to be up to date.
pub fn upgrade_plan(
    wanted: &[&str],
    found: &HashMap<&'static str, Found>,
    latest: &HashMap<&'static str, String>,
) -> Vec<&'static Tool> {
    TOOLS
        .iter()
        .filter(|t| found.contains_key(t.id))
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
    run_script(tool.name, tool.install, on_line).await
}

/// Runs `command` (`tool`'s installer or upgrade) like [`install`].
pub async fn run(tool: &Tool, command: &str, on_line: impl Fn(String) + Send + Sync) -> Result<()> {
    run_script(tool.name, command, on_line).await
}

/// How to remove the copy of `tool` found at `found`: `brew uninstall`
/// when Homebrew put it there, the tool's own command when its installer
/// did (under the home folder, or at one of its known locations), and
/// `None` for anything else, which Sawit leaves alone.
pub fn uninstall_command(tool: &Tool, found: &std::path::Path) -> Option<String> {
    let real = std::fs::canonicalize(found).unwrap_or_else(|_| found.to_path_buf());
    let home = PathBuf::from(std::env::var("HOME").unwrap_or_default());
    uninstall_command_in(tool, found, &real, &home)
}

fn uninstall_command_in(
    tool: &Tool,
    found: &std::path::Path,
    real: &std::path::Path,
    home: &std::path::Path,
) -> Option<String> {
    let parts: Vec<_> = real
        .components()
        .map(|c| c.as_os_str().to_string_lossy())
        .collect();
    if tool.id != "homebrew"
        && let Some(ix) = parts.iter().position(|p| p == "Cellar")
        && let Some(formula) = parts.get(ix + 1)
    {
        return Some(format!("brew uninstall {formula}"));
    }
    let ours = found.starts_with(home)
        || real.starts_with(home)
        || tool.also.iter().any(|p| expand(p) == found);
    ours.then_some(tool.uninstall).flatten().map(String::from)
}

/// Runs an uninstall `command` for `tool`, like [`install`].
pub async fn uninstall(
    tool: &Tool,
    command: &str,
    on_line: impl Fn(String) + Send + Sync,
) -> Result<()> {
    run_script(tool.name, command, on_line).await
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
    let script = format!("{PRELUDE}set -e\n{command}\n");
    let mut cmd = shell();
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
            let mut lines = BufReader::new(pipe).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                let line = strip_ansi(&line);
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
        assert_eq!(ids, ["npm", "gemini"]);
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
        for id in ["bun", "deno", "pnpm", "claude-code"] {
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
