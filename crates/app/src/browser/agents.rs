//! Where the Annotate panel's notes go: to a coding agent running in one of Sawit's terminals, as
//! a message in its chat, or to a new one started with the notes as its first message. The panel
//! shows the choice; the workspace, which owns the terminals, sends.

use std::path::{Path, PathBuf};
use std::rc::Rc;

use gpui_kit::*;
use sawit_core::terminal::{AgentKind, PaneId, shell_quote};

use crate::terminal::RunningAgent;

/// The agents running in the terminals, as the workspace finds them.
pub type AgentSource = Rc<dyn Fn(&App) -> Vec<RunningAgent>>;

/// Whether Open agent starts the CLI in print mode (`claude -p`, `codex exec`, `gemini -p`,
/// `opencode run`): it answers once and exits, and can't ask before it edits a file, so a Fix
/// note couldn't be acted on. Off, the session stays open with the notes as its first message,
/// and the user can answer its questions and go on in the same chat.
pub const PRINT_MODE: bool = false;

/// The arguments before the prompt that start the CLI `id` with a first message, in print mode
/// or not; `None` for a CLI whose way of taking one isn't known. Each CLI's `--help` and
/// reference say so: Claude Code's prompt argument starts an interactive session (`-p` prints);
/// so does Codex's (`codex exec` runs without one); Gemini CLI's `-i` runs the prompt and stays
/// interactive (`-p` doesn't); OpenCode's TUI takes `--prompt` (`opencode run` doesn't stay).
fn prompt_args(id: &str, print: bool) -> Option<&'static [&'static str]> {
    Some(match (id, print) {
        ("claude-code", false) => &[],
        ("claude-code", true) => &["-p"],
        ("codex", false) => &[],
        ("codex", true) => &["exec"],
        ("gemini", false) => &["-i"],
        ("gemini", true) => &["-p"],
        ("opencode", false) => &["--prompt"],
        ("opencode", true) => &["run"],
        _ => return None,
    })
}

/// Whether Open agent knows how to start `kind` with the notes.
pub fn can_open(kind: AgentKind) -> bool {
    prompt_args(kind.id(), PRINT_MODE).is_some()
}

/// The command line, typed into a new terminal's shell, that starts `kind` with the prompt in
/// `file` as its first message. The notes run to many lines with quotes, backticks and `$` in
/// them, which no quoting on a command line typed into a shell keeps safe, so the shell reads
/// them from the file instead: `"$(cat -- file)"` is one argument, whatever the file holds, in
/// zsh, bash, sh and fish alike.
pub fn open_command(kind: AgentKind, file: &Path) -> Option<String> {
    open_command_with(kind, file, PRINT_MODE)
}

fn open_command_with(kind: AgentKind, file: &Path, print: bool) -> Option<String> {
    let mut words = vec![kind.executable().to_string()];
    words.extend(prompt_args(kind.id(), print)?.iter().map(|a| a.to_string()));
    words.push(format!("\"$(cat -- {})\"", shell_quote(file)));
    Some(words.join(" "))
}

/// The agent CLIs on this Mac that Open agent can start, found where the Dev Tools page finds
/// them. Reads the disk: call it off the UI thread.
pub fn installed_agents() -> Vec<AgentKind> {
    AgentKind::all()
        .filter(|k| can_open(*k) && sawit_core::dev_tools::find_program(k.executable()).is_some())
        .collect()
}

/// Which of `agents` the send button goes to: the one the user types to (the pane with the
/// keyboard, or that had it last), else the one last sent to while it still runs (`last`), else
/// the newest.
pub fn default_target(agents: &[RunningAgent], last: Option<PaneId>) -> Option<usize> {
    agents
        .iter()
        .position(|a| a.focused)
        .or_else(|| last.and_then(|pane| agents.iter().position(|a| a.pane == pane)))
        .or_else(|| agents.len().checked_sub(1))
}

/// The CLI Open agent starts: the one last used, else Claude Code, else the first one found.
pub fn default_open(installed: &[AgentKind], last: Option<AgentKind>) -> Option<AgentKind> {
    last.filter(|k| installed.contains(k))
        .or_else(|| installed.iter().copied().find(|k| k.id() == "claude-code"))
        .or_else(|| installed.first().copied())
}

/// "Claude Code · sawit.dev": the agent and what its terminal is called. Agents of one kind in
/// terminals called alike are numbered in the terminals' order: "Claude Code · zsh (2)".
pub fn target_name(agents: &[RunningAgent], ix: usize) -> String {
    let a = &agents[ix];
    let twin = |b: &RunningAgent| b.kind == a.kind && b.label == a.label;
    let name = format!("{} · {}", a.kind.name(), a.label);
    if agents.iter().filter(|b| twin(b)).count() > 1 {
        let n = agents[..ix].iter().filter(|b| twin(b)).count() + 1;
        format!("{name} ({n})")
    } else {
        name
    }
}

/// Writes the notes for a new agent's first message beside the day's screenshots: the project's
/// `.nyawit/annotations/<date>/agent-prompt.md` (`-2`, `-3`… after the first), else the app
/// cache's. Its path.
pub fn write_prompt(project: Option<&Path>, markdown: &str) -> Result<PathBuf, String> {
    let cache = sawit_core::instances::AppPaths::default_macos().cache_dir();
    let dir =
        sawit_core::annotations::screenshot_dir(project, &cache, &sawit_core::date::local_date());
    match project {
        Some(project) => sawit_core::nyawit::ensure(project, &dir),
        None => std::fs::create_dir_all(&dir),
    }
    .and_then(|()| sawit_core::nyawit::write_unique(&dir, "agent-prompt.md", markdown.as_bytes()))
    .map_err(|e| format!("Couldn't write the notes to {}: {e}", dir.display()))
}

/// What tells two lists of agents apart for the send button: which agents, where, called what,
/// and which one the user types to. (The terminal's own list holds handles that can't be
/// compared for that.)
pub fn same_agents(a: &[RunningAgent], b: &[RunningAgent]) -> bool {
    let key = |r: &RunningAgent| (r.pane, r.kind, r.label.clone(), r.focused);
    a.len() == b.len() && a.iter().zip(b).all(|(a, b)| key(a) == key(b))
}

#[cfg(test)]
mod tests {
    use super::{
        AgentKind, can_open, default_open, default_target, open_command_with, prompt_args,
        same_agents, target_name,
    };
    use crate::terminal::RunningAgent;
    use std::path::Path;

    /// The CLIs Open agent knows how to start with a first message.
    const OPENABLE: [AgentKind; 4] = [
        AgentKind::ClaudeCode,
        AgentKind::Codex,
        AgentKind::GeminiCli,
        AgentKind::Opencode,
    ];

    fn agent(pane: u64, kind: AgentKind, label: &str, focused: bool) -> RunningAgent {
        RunningAgent {
            tab: gpui_kit::WeakEntity::new_invalid(),
            pane,
            kind,
            label: label.to_string().into(),
            focused,
        }
    }

    /// The send button goes to the agent in front, else the one last sent to while it runs,
    /// else the newest.
    #[test]
    fn the_default_target_is_in_front_else_the_last_used() {
        use AgentKind::{ClaudeCode, Codex};
        let agents = [
            agent(1, ClaudeCode, "sawit.dev", false),
            agent(2, Codex, "api", false),
            agent(3, ClaudeCode, "web", false),
        ];
        assert_eq!(default_target(&agents, None), Some(2), "the newest");
        assert_eq!(default_target(&agents, Some(1)), Some(0), "the last used");
        assert_eq!(
            default_target(&agents, Some(9)),
            Some(2),
            "the last used has exited"
        );
        let mut front = agents.clone();
        front[1].focused = true;
        assert_eq!(default_target(&front, Some(1)), Some(1), "the one in front");
        assert_eq!(default_target(&[], Some(1)), None);
    }

    /// Open agent starts the one last used while it is installed, else Claude Code.
    #[test]
    fn open_agent_defaults_to_the_last_used_else_claude_code() {
        use AgentKind::{ClaudeCode, Codex, GeminiCli};
        assert_eq!(default_open(&[Codex, ClaudeCode], None), Some(ClaudeCode));
        assert_eq!(default_open(&[Codex, ClaudeCode], Some(Codex)), Some(Codex));
        assert_eq!(default_open(&[Codex], Some(GeminiCli)), Some(Codex));
        assert_eq!(default_open(&[], Some(GeminiCli)), None);
    }

    /// Two agents of one kind in terminals called alike are numbered in the terminals' order.
    #[test]
    fn twins_are_numbered() {
        use AgentKind::{ClaudeCode, Codex};
        let agents = [
            agent(0, ClaudeCode, "sawit.dev", false),
            agent(1, Codex, "sawit.dev", false),
            agent(2, ClaudeCode, "sawit.dev", false),
        ];
        assert_eq!(target_name(&agents, 0), "Claude Code · sawit.dev (1)");
        assert_eq!(target_name(&agents, 1), "Codex · sawit.dev");
        assert_eq!(target_name(&agents, 2), "Claude Code · sawit.dev (2)");
    }

    /// The button redraws when an agent starts, ends, is renamed or takes the keyboard.
    #[test]
    fn lists_of_agents_compare_by_what_the_button_shows() {
        use AgentKind::{ClaudeCode, Codex};
        let one = [agent(1, ClaudeCode, "web", false)];
        assert!(same_agents(&one, &[agent(1, ClaudeCode, "web", false)]));
        assert!(!same_agents(&one, &[agent(1, ClaudeCode, "web", true)]));
        assert!(!same_agents(&one, &[agent(1, Codex, "web", false)]));
        assert!(!same_agents(&one, &[]));
    }

    /// Each CLI starts interactive with the notes as its first message: Claude Code and Codex
    /// take it as their argument, Gemini CLI after `-i`, OpenCode after `--prompt`. The file's
    /// path is quoted for the shell, and print mode is one switch away.
    #[test]
    fn open_commands_start_each_cli_with_the_notes() {
        let file = Path::new("/p/.nyawit/annotations/2026-11-01/agent-prompt.md");
        let open = |kind, print| open_command_with(kind, file, print).unwrap();
        let read = "\"$(cat -- /p/.nyawit/annotations/2026-11-01/agent-prompt.md)\"";
        assert_eq!(open(AgentKind::ClaudeCode, false), format!("claude {read}"));
        assert_eq!(open(AgentKind::Codex, false), format!("codex {read}"));
        assert_eq!(
            open(AgentKind::GeminiCli, false),
            format!("gemini -i {read}")
        );
        assert_eq!(
            open(AgentKind::Opencode, false),
            format!("opencode --prompt {read}")
        );
        assert_eq!(
            open(AgentKind::ClaudeCode, true),
            format!("claude -p {read}")
        );
        assert_eq!(open(AgentKind::Codex, true), format!("codex exec {read}"));
        assert_eq!(
            open(AgentKind::GeminiCli, true),
            format!("gemini -p {read}")
        );
        assert_eq!(
            open(AgentKind::Opencode, true),
            format!("opencode run {read}")
        );
        assert!(OPENABLE.into_iter().all(can_open) && !can_open(AgentKind::Amp));
        assert_eq!(prompt_args("amp", false), None);

        let odd = Path::new("/Users/me/it's here/agent-prompt.md");
        assert_eq!(
            open_command_with(AgentKind::ClaudeCode, odd, false).unwrap(),
            r#"claude "$(cat -- '/Users/me/it'\''s here/agent-prompt.md')""#
        );
    }

    /// What a login shell hands the CLI is the file's text as one argument, to the byte: lines,
    /// quotes, backticks and `$` are not run or split, in sh and zsh. Each CLI is a function here
    /// that prints its arguments, one per line, between markers.
    #[test]
    fn the_shell_hands_the_notes_over_as_one_argument() {
        let dir = crate::testing::scratch_dir("agent-prompt");
        let dir = dir.join("it's a \"dir\" $HOME");
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("agent-prompt.md");
        let notes = "## Page `/` — \"Home\"\n\n- **Fix:** it's $HOME and `whoami`; $(echo no)\n\t* done \\n\n";
        std::fs::write(&file, notes).unwrap();
        let echo = "for a in \"$@\"; do printf '<%s>\\n' \"$a\"; done";
        let functions = OPENABLE
            .iter()
            .map(|k| format!("{}() {{ {echo}; }}", k.executable()))
            .collect::<Vec<_>>()
            .join("\n");
        // `$(…)` drops the file's trailing newlines, as it does for every command.
        let body = notes.trim_end_matches('\n');
        for shell in ["/bin/sh", "/bin/zsh"] {
            if !Path::new(shell).exists() {
                continue;
            }
            // zsh without the user's startup files.
            let quiet: &[&str] = if shell.ends_with("zsh") { &["-f"] } else { &[] };
            for kind in OPENABLE {
                let command = open_command_with(kind, &file, false).unwrap();
                let out = std::process::Command::new(shell)
                    .args(quiet)
                    .arg("-c")
                    .arg(format!("{functions}\n{command}"))
                    .env_remove("ENV")
                    .output()
                    .unwrap();
                let flags: String = prompt_args(kind.id(), false)
                    .unwrap()
                    .iter()
                    .map(|f| format!("<{f}>\n"))
                    .collect();
                assert_eq!(
                    String::from_utf8_lossy(&out.stdout),
                    format!("{flags}<{body}>\n"),
                    "{shell}, {}",
                    kind.name()
                );
            }
        }
    }
}
