# Plan 002: Agent detection no longer panics (and kills the app) on a non-ASCII script or package name

> **Executor instructions**: Follow this plan step by step. Run every
> verification command and confirm the expected result before moving to the
> next step. If anything in the "STOP conditions" section occurs, stop and
> report — do not improvise. When done, update the status row for this plan
> in `plans/README.md` — unless a reviewer dispatched you and told you they
> maintain the index.
>
> **Drift check (run first)**: `git diff --stat 7e715d73..HEAD -- crates/core/src/terminal/agents`
> If any in-scope file changed since this plan was written, compare the
> "Current state" excerpts against the live code before proceeding; on a
> mismatch, treat it as a STOP condition.

## Status

- **Priority**: P1
- **Effort**: S
- **Risk**: LOW
- **Depends on**: none
- **Category**: bug
- **Planned at**: commit `7e715d73`, 2026-11-16

## Why this matters

Sawit is a macOS developer app with a built-in terminal. Whenever the terminal's foreground process
changes, Sawit guesses whether it is an AI coding agent (Claude Code, Codex, …) from its command
line. When the program is a launcher (`node`, `python3`, `bunx`, …), its first argument goes
through `by_package`, which slices the string at **byte** 1. If the first character is multi-byte
(`ñ.js`, `日本.py`, `ü`), that slice is not on a char boundary and Rust panics. The call runs on the
UI thread, so typing `node ñ.js` in any Sawit terminal crashes the whole app, taking every terminal
and dev server with it.

## Current state

- `crates/core/src/terminal/agents/command.rs:160-173`:
  ```rust
  /// The agent an npm or PyPI package spec names: `@openai/codex`,
  /// `@anthropic-ai/claude-code@latest`, `npm:@google/gemini-cli`, `aider-chat`.
  pub(super) fn by_package(spec: &str) -> Option<AgentKind> {
      let spec = spec.strip_prefix("npm:").unwrap_or(spec);
      let spec = match spec[1.min(spec.len())..].find('@') {
          Some(at) => &spec[..at + 1],
          None => spec,
      };
      let spec = spec.to_ascii_lowercase();
      KNOWN
          .iter()
          .find(|k| k.packages.contains(&spec.as_str()))
          .map(|k| k.agent)
  }
  ```
  Intent: skip the first character (a scope's leading `@`), then cut at the next `@` (the version).
- `crates/core/src/terminal/agents/mod.rs:126-130` — the path that reaches it:
  ```rust
  let (target, rest) = launched(launcher, args)?;
  by_command(target, rest)
      .or_else(|| by_install_path(target))
      .or_else(|| by_package(target))
  ```
- Tests live in `crates/core/src/terminal/agents/tests.rs` (406 lines; `use super::*;`), with
  helpers at the top:
  ```rust
  fn argv(words: &[&str]) -> Vec<String> { … }
  fn agent(exec: &str, words: &[&str]) -> Option<AgentKind> {
      AgentKind::from_process(exec, &argv(words))
  }
  ```
  and tests like `a_package_runner_is_known_by_the_package` (line ~147) asserting e.g.
  `agent(node, &["node", "/opt/homebrew/bin/npx", "-y", "@anthropic-ai/claude-code@latest"]) == Some(AgentKind::ClaudeCode)`.
  Test names are plain-English sentences; doc comments explain the case. Match that.
- Other byte-slicing in the same folder may have the same bug. Check with
  `grep -n "\[[0-9]\.\.\|\[1\.min\|\.\.[0-9]\]" crates/core/src/terminal/agents/*.rs`.

## Commands you will need

**Environment (mandatory for every cargo command):** prefix with
```
source /Volumes/bahrul/.toolchains/env.sh && export CARGO_TARGET_DIR=/Volumes/bahrul/project/sawit-advisor-target &&
```
Never run cargo without it. Run cargo in the Sawit app's terminal, not a hidden shell: the `sawit`
MCP tool `terminal_run` (load via ToolSearch `select:mcp__sawit__terminal_run`), `new_tab: true`,
`cwd` = your worktree root, command ending in
`2>&1 | tee /Volumes/bahrul/.toolchains/logs/advisor-002.log; echo "exit=${pipestatus[1]}" > /Volumes/bahrul/.toolchains/logs/advisor-002.done`
(zsh). Wait for the `.done` file (Monitor tool, or Bash `test -f` every minute), then read the log.
Delete a stale `.done` before reusing the name. Never type into a tab you didn't just create.
Builds share a target dir with other executors and may wait on cargo's file lock; the first build
can take a long time. That's normal.

| Purpose | Command (after the env prefix) | Expected |
|---|---|---|
| Tests | `cargo test -p sawit-core --lib terminal::agents` | all pass |
| Clippy | `cargo clippy -p sawit-core --all-targets -- -D warnings` | exit 0 |
| Line limit | `sh scripts/check-line-limit.sh crates/core/src/terminal` | all ≤ 500 |

## Scope

**In scope**: `crates/core/src/terminal/agents/command.rs`, `crates/core/src/terminal/agents/tests.rs`
(and another file in that folder only if the grep finds the same byte-slice bug there).

**Out of scope**: `crates/app/**` (the caller is fine once core doesn't panic); any change to which
agents are detected; formatting other files.

## Git workflow

Commit on your worktree's branch, e.g.
`fix(terminal): a script or package name that starts with a non-ASCII letter no longer crashes the app`.
Do NOT push.

## Steps

### Step 1: Write the failing test first

In `tests.rs`, add a test (e.g. `a_non_ascii_script_or_package_is_no_agent`) with a doc comment,
asserting `None` (not a panic) for at least:
- `agent("/opt/homebrew/bin/node", &["node", "ñ.js"])`
- `agent("/usr/bin/python3", &["python3", "日本.py"])`
- `agent("/opt/homebrew/bin/bunx", &["bunx", "ü"])`
- `agent("/opt/homebrew/bin/node", &["node", "/opt/homebrew/bin/npx", "é@1.0"])`

**Verify**: the test command → the new test FAILS with a panic mentioning "byte index" / "char
boundary". (If it passes, STOP: the bug isn't reachable as described.)

### Step 2: Fix `by_package`

Skip the first *character*, not byte. Target shape:
```rust
let first = spec.chars().next().map_or(0, char::len_utf8);
let spec = match spec[first..].find('@') {
    Some(at) => &spec[..first + at],
    None => spec,
};
```
Note the cut index must be `first + at` (the old `at + 1` equals this only for a 1-byte first
char). Fix any other byte-slice the Step-0 grep found the same way.

**Verify**: test command → all pass (new test and the existing `a_package_runner_is_known_by_the_package`).

### Step 3: Lint and limits

**Verify**: clippy → exit 0; line-limit → passes. If `rustfmt --edition 2024 --check <file>`
passed on a file *before* your edit, run `rustfmt --edition 2024 <file>` on it after; otherwise
don't reformat it.

## Test plan

New test in `tests.rs` (Step 1), covering node script, python script, a bare package runner arg,
and a package spec with a version. Existing ASCII tests must keep passing.

## Done criteria

- [ ] `cargo test -p sawit-core --lib terminal::agents` passes, including the new test
- [ ] `grep -n "1.min(spec.len())" crates/core/src/terminal/agents/command.rs` → no match
- [ ] `cargo clippy -p sawit-core --all-targets -- -D warnings` exits 0
- [ ] `git status` shows only in-scope files changed

## STOP conditions

- The excerpt doesn't match the live code.
- The new test passes before the fix (the panic path is different from described).
- A fix would need changes in `crates/app`.

## Maintenance notes

Any future string slicing on process arguments must use char-aware indices; arguments are
user-controlled text. Reviewer: check the `first + at` arithmetic against the scoped-package test
cases (`@openai/codex`, `@anthropic-ai/claude-code@latest`).
