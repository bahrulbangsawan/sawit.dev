//! The catalog: every tool the Dev Tools page offers, the command that installs it, upgrades it
//! and removes it, and where it lands.

use super::{Category, Latest, Tool};

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
