use super::*;

fn argv(words: &[&str]) -> Vec<String> {
    words.iter().map(|w| w.to_string()).collect()
}

fn agent(exec: &str, words: &[&str]) -> Option<AgentKind> {
    AgentKind::from_process(exec, &argv(words))
}

#[test]
fn a_native_binary_is_known_by_its_name() {
    assert_eq!(
        agent("/opt/homebrew/bin/codex", &["codex"]),
        Some(AgentKind::Codex)
    );
    assert_eq!(
        agent("/usr/local/bin/goose", &["goose"]),
        Some(AgentKind::Goose)
    );
    assert_eq!(
        agent("/Users/me/.local/bin/cursor-agent", &["cursor-agent"]),
        Some(AgentKind::CursorAgent)
    );
    assert_eq!(
        agent("/Users/me/.local/bin/kiro-cli", &["kiro-cli", "chat"]),
        Some(AgentKind::Kiro)
    );
    assert_eq!(agent("/usr/bin/vim", &["vim", "notes.md"]), None);
}

/// The shell runs a command by the path it found, so argv[0] may be a full path.
#[test]
fn a_path_is_known_by_its_last_part() {
    assert_eq!(
        agent(
            "/Users/me/.local/share/claude/versions/2.0.14",
            &["/Users/me/.local/bin/claude", "--resume"],
        ),
        Some(AgentKind::ClaudeCode)
    );
    assert_eq!(
        agent(
            "/opt/homebrew/bin/opencode",
            &["/opt/homebrew/bin/opencode"]
        ),
        Some(AgentKind::Opencode)
    );
}

/// Claude Code's native install runs a binary named after its version, from a `versions`
/// folder of its own: run under another name, the folder still says what it is.
#[test]
fn a_version_named_binary_is_known_by_its_folder_or_suffix() {
    assert_eq!(
        agent("/Users/me/.local/share/claude/versions/2.0.14", &["2.0.14"]),
        Some(AgentKind::ClaudeCode)
    );
    assert_eq!(
        agent(
            "/tmp/codex-aarch64-apple-darwin",
            &["codex-aarch64-apple-darwin"]
        ),
        Some(AgentKind::Codex)
    );
    assert_eq!(
        agent("/tmp/claude-2.0.14", &["claude-2.0.14"]),
        Some(AgentKind::ClaudeCode)
    );
    assert_eq!(
        agent("/tmp/gemini@0.9.0", &["gemini@0.9.0"]),
        Some(AgentKind::GeminiCli)
    );
    assert_eq!(command_name("/x/Codex-X86_64-Apple-Darwin"), "codex");
    assert_eq!(command_name("kiro-cli"), "kiro-cli");
    assert_eq!(command_name("cursor-agent"), "cursor-agent");
}

/// An npm-installed agent is a Node script: the process is `node`, and the script it runs
/// (by the bin link the shell found, or inside the package) names the agent.
#[test]
fn a_node_script_is_known_by_the_script() {
    assert_eq!(
        agent(
            "/opt/homebrew/Cellar/node/24.1.0/bin/node",
            &["node", "/opt/homebrew/bin/claude"],
        ),
        Some(AgentKind::ClaudeCode)
    );
    assert_eq!(
        agent(
            "/opt/homebrew/bin/node",
            &[
                "node",
                "--no-warnings",
                "--enable-source-maps",
                "/opt/homebrew/lib/node_modules/@openai/codex/bin/codex.js",
            ],
        ),
        Some(AgentKind::Codex)
    );
    assert_eq!(
        agent(
            "/opt/homebrew/bin/node",
            &[
                "node",
                "/opt/homebrew/lib/node_modules/@anthropic-ai/claude-code/cli.js",
            ],
        ),
        Some(AgentKind::ClaudeCode)
    );
    assert_eq!(
        agent(
            "/Users/me/.bun/bin/bun",
            &[
                "bun",
                "/Users/me/.bun/install/global/node_modules/@google/gemini-cli/dist/index.js"
            ],
        ),
        Some(AgentKind::GeminiCli)
    );
    // A script's own arguments never name what runs.
    assert_eq!(
        agent("/opt/homebrew/bin/node", &["node", "server.js", "codex"]),
        None
    );
    assert_eq!(agent("/opt/homebrew/bin/node", &["node", "-e", "1"]), None);
    assert_eq!(agent("/opt/homebrew/bin/node", &["node"]), None);
}

/// Cursor's agent is a script that starts its own bundled `node` from a versions folder.
#[test]
fn a_bundled_runtime_is_known_by_its_folder() {
    assert_eq!(
        agent(
            "/Users/me/.local/share/cursor-agent/versions/2025.10.02-bd871ac/node",
            &[
                "/Users/me/.local/share/cursor-agent/versions/2025.10.02-bd871ac/node",
                "/Users/me/.local/share/cursor-agent/versions/2025.10.02-bd871ac/index.js",
            ],
        ),
        Some(AgentKind::CursorAgent)
    );
}

#[test]
fn a_package_runner_is_known_by_the_package() {
    let node = "/opt/homebrew/bin/node";
    assert_eq!(
        agent(
            node,
            &[
                "node",
                "/opt/homebrew/bin/npx",
                "-y",
                "@anthropic-ai/claude-code@latest"
            ]
        ),
        Some(AgentKind::ClaudeCode)
    );
    assert_eq!(
        agent(
            node,
            &["node", "/opt/homebrew/bin/pnpm", "dlx", "@openai/codex"]
        ),
        Some(AgentKind::Codex)
    );
    assert_eq!(
        agent(
            "/Users/me/.local/bin/uvx",
            &["uvx", "--from", "aider-chat", "aider"]
        ),
        Some(AgentKind::Aider)
    );
    assert_eq!(
        agent(
            "/opt/homebrew/bin/deno",
            &["deno", "run", "-A", "npm:@google/gemini-cli"]
        ),
        Some(AgentKind::GeminiCli)
    );
    assert_eq!(
        agent("/usr/bin/caffeinate", &["caffeinate", "-i", "claude"]),
        Some(AgentKind::ClaudeCode)
    );
    assert_eq!(
        agent("/usr/bin/env", &["env", "DEBUG=1", "codex", "--full-auto"]),
        Some(AgentKind::Codex)
    );
}

/// Aider is Python: the process is the interpreter, running its entry script or module.
#[test]
fn a_python_script_is_known_by_the_script_or_module() {
    assert_eq!(
        agent(
            "/opt/homebrew/Cellar/python@3.12/3.12.7/Frameworks/Python.framework/Versions/3.12/Resources/Python.app/Contents/MacOS/Python",
            &[
                "/Users/me/.local/pipx/venvs/aider-chat/bin/python",
                "/Users/me/.local/bin/aider"
            ],
        ),
        Some(AgentKind::Aider)
    );
    assert_eq!(
        agent(
            "/usr/bin/python3",
            &["python3", "-m", "aider", "--model", "x"]
        ),
        Some(AgentKind::Aider)
    );
    // Python's -u is a flag, env's takes the name it unsets.
    assert_eq!(
        agent(
            "/usr/bin/python3",
            &["python3", "-u", "/Users/me/.local/bin/aider"]
        ),
        Some(AgentKind::Aider)
    );
    assert_eq!(
        agent("/usr/bin/env", &["env", "-u", "TMUX", "claude"]),
        Some(AgentKind::ClaudeCode)
    );
}

/// The process decides; a title the job set only stands in when the process says nothing.
#[test]
fn detect_takes_the_process_before_the_title() {
    let ssh = ProcessArgs {
        exec_path: "/usr/bin/ssh".into(),
        argv: argv(&["ssh", "devbox"]),
    };
    let codex = ProcessArgs {
        exec_path: "/opt/homebrew/bin/codex".into(),
        argv: argv(&["codex"]),
    };
    assert_eq!(
        detect(Some(&ssh), Some("✳ Claude Code")),
        Some(AgentKind::ClaudeCode)
    );
    assert_eq!(detect(Some(&ssh), Some("devbox")), None);
    assert_eq!(detect(Some(&ssh), None), None);
    assert_eq!(
        detect(Some(&codex), Some("✳ Claude Code")),
        Some(AgentKind::Codex)
    );
    assert_eq!(
        AgentKind::detect(None, Some("✳ Claude Code")),
        Some(AgentKind::ClaudeCode)
    );
}

/// Every logo named is a file in assets/agent_logos, and each agent's command is one it is
/// known by.
#[test]
fn logos_and_executables_are_real() {
    let logos = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/agent_logos");
    for kind in AgentKind::all() {
        if let Some(logo) = kind.logo() {
            let file = std::path::Path::new(logos).join(format!("{logo}.svg"));
            assert!(file.is_file(), "{} has no {}", kind.name(), file.display());
        }
        let exe = kind.executable();
        assert_eq!(by_command(exe, &[]), Some(kind), "{exe}");
    }
    assert_eq!(AgentKind::ClaudeCode.executable(), "claude");
    assert_eq!(AgentKind::GeminiCli.executable(), "gemini");
    assert_eq!(AgentKind::ClaudeCode.logo(), Some("claudecode-color"));
}

/// Commands another tool shares: the migration tool `goose` and AWS's `copilot` always take
/// a subcommand of their own, the agents a flag or nothing.
#[test]
fn a_shared_command_name_needs_the_agents_own_start() {
    assert_eq!(
        agent("/opt/homebrew/bin/goose", &["goose", "session"]),
        Some(AgentKind::Goose)
    );
    assert_eq!(
        agent("/opt/homebrew/bin/goose", &["goose", "postgres", "x", "up"]),
        None
    );
    assert_eq!(agent("/opt/homebrew/bin/goose", &["goose", "up"]), None);
    assert_eq!(
        agent("/opt/homebrew/bin/copilot", &["copilot"]),
        Some(AgentKind::Copilot)
    );
    assert_eq!(
        agent(
            "/opt/homebrew/bin/copilot",
            &["copilot", "--allow-all-tools"]
        ),
        Some(AgentKind::Copilot)
    );
    assert_eq!(
        agent("/opt/homebrew/bin/copilot", &["copilot", "svc", "deploy"]),
        None
    );
}

#[test]
fn every_requested_agent_is_known_by_its_command() {
    for (command, expected) in [
        ("claude", AgentKind::ClaudeCode),
        ("codex", AgentKind::Codex),
        ("gemini", AgentKind::GeminiCli),
        ("opencode", AgentKind::Opencode),
        ("amp", AgentKind::Amp),
        ("cursor-agent", AgentKind::CursorAgent),
        ("copilot", AgentKind::Copilot),
        ("goose", AgentKind::Goose),
        ("qwen", AgentKind::Qwen),
        ("kiro-cli", AgentKind::Kiro),
        ("junie", AgentKind::Junie),
        ("aider", AgentKind::Aider),
        ("crush", AgentKind::Crush),
        ("droid", AgentKind::Droid),
        ("warp", AgentKind::Warp),
        ("cline", AgentKind::Cline),
        ("kilocode", AgentKind::KiloCode),
        ("omp", AgentKind::OhMyPi),
    ] {
        assert_eq!(agent(&format!("/x/{command}"), &[command]), Some(expected));
    }
}

#[test]
fn ids_and_names_are_unique() {
    let all: Vec<AgentKind> = AgentKind::all().collect();
    for (i, a) in all.iter().enumerate() {
        for b in &all[i + 1..] {
            assert_ne!(a.id(), b.id());
            assert_ne!(a.name(), b.name());
        }
    }
    assert_eq!(AgentKind::ClaudeCode.id(), "claude-code");
}

#[test]
fn a_title_names_the_agent_only_at_its_start() {
    assert_eq!(
        AgentKind::from_title("✳ Claude Code"),
        Some(AgentKind::ClaudeCode)
    );
    assert_eq!(
        AgentKind::from_title("⠂ Claude Code"),
        Some(AgentKind::ClaudeCode)
    );
    assert_eq!(
        AgentKind::from_title("codex --full-auto"),
        Some(AgentKind::Codex)
    );
    assert_eq!(
        AgentKind::from_title("Gemini CLI (sawit.dev)"),
        Some(AgentKind::GeminiCli)
    );
    assert_eq!(AgentKind::from_title("~/projects/codex"), None);
    assert_eq!(AgentKind::from_title("vim codex.rs"), None);
    assert_eq!(AgentKind::from_title("codex-rs"), None);
    assert_eq!(AgentKind::from_title("Ampersand"), None);
    assert_eq!(
        AgentKind::from_title("goose up"),
        None,
        "the migration tool"
    );
    assert_eq!(AgentKind::from_title("zsh"), None);
    assert_eq!(AgentKind::from_title(""), None);
}

#[test]
fn names_read_as_a_sentence() {
    use AgentKind::*;
    assert_eq!(names_phrase(&[]), "");
    assert_eq!(names_phrase(&[ClaudeCode]), "Claude Code");
    assert_eq!(names_phrase(&[ClaudeCode, Codex]), "Claude Code and Codex");
    assert_eq!(
        names_phrase(&[ClaudeCode, Codex, GeminiCli]),
        "Claude Code, Codex and Gemini CLI"
    );
}

#[test]
fn procargs_give_the_executable_and_arguments() {
    let mut buf = 2i32.to_ne_bytes().to_vec();
    buf.extend_from_slice(b"/opt/homebrew/bin/node\0\0\0\0");
    buf.extend_from_slice(b"node\0/opt/homebrew/bin/claude\0");
    buf.extend_from_slice(b"HOME=/Users/me\0");
    assert_eq!(
        parse_procargs(&buf),
        Some(ProcessArgs {
            exec_path: "/opt/homebrew/bin/node".into(),
            argv: argv(&["node", "/opt/homebrew/bin/claude"]),
        })
    );
    assert_eq!(parse_procargs(&[1, 0]), None);
}

/// This test's own process can be read, as a terminal's foreground job is.
#[cfg(target_os = "macos")]
#[test]
fn reads_a_live_process() {
    let me = process_args(std::process::id()).expect("its own arguments");
    assert!(!me.argv.is_empty());
    assert!(me.exec_path.starts_with('/'), "{}", me.exec_path);
    assert!(process_name(std::process::id()).is_some());
}
