//! Starting a coding agent's CLI with a first message: the arguments each takes before its prompt,
//! and the command line that reads the prompt from a file. The app's Open agent and Send to Agent
//! use it, and so does `sawit-mcp`'s canvas_handoff, which opens the agent in a new terminal tab.

use std::path::Path;

use super::AgentKind;
use crate::terminal::shell_quote;

/// The arguments before the prompt that start the CLI `id` with a first message, in print mode
/// or not; `None` for a CLI whose way of taking one isn't known. Each CLI's `--help` and
/// reference say so: Claude Code's prompt argument starts an interactive session (`-p` prints);
/// so does Codex's (`codex exec` runs without one); Gemini CLI's `-i` runs the prompt and stays
/// interactive (`-p` doesn't); OpenCode's TUI takes `--prompt` (`opencode run` doesn't stay).
pub fn prompt_args(id: &str, print: bool) -> Option<&'static [&'static str]> {
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

/// The command line, typed into a new terminal's shell, that starts `kind` with the prompt in
/// `file` as its first message: `"$(cat -- file)"` is one argument whatever the file holds, in
/// zsh, bash, sh and fish alike. `None` for a CLI that takes no first message.
pub fn start_with_prompt(kind: AgentKind, file: &Path, print: bool) -> Option<String> {
    let mut words = vec![kind.executable().to_string()];
    words.extend(prompt_args(kind.id(), print)?.iter().map(|a| a.to_string()));
    words.push(format!("\"$(cat -- {})\"", shell_quote(file)));
    Some(words.join(" "))
}
