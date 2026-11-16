# Plan 004: Copy Last Output and `terminal_read output:"last"` return the output of a command that is still running

> **Executor instructions**: Follow this plan step by step. Run every
> verification command and confirm the expected result before moving to the
> next step. If anything in the "STOP conditions" section occurs, stop and
> report — do not improvise. When done, update the status row for this plan
> in `plans/README.md` — unless a reviewer dispatched you and told you they
> maintain the index.
>
> **Drift check (run first)**: `git diff --stat 7e715d73..HEAD -- crates/core/src/terminal/session/text.rs crates/app/src/terminal/view/copy_out.rs`
> If any in-scope file changed since this plan was written, compare the
> "Current state" excerpts against the live code before proceeding; on a
> mismatch, treat it as a STOP condition.

## Status

- **Priority**: P2
- **Effort**: S
- **Risk**: LOW
- **Depends on**: none
- **Category**: bug
- **Planned at**: commit `7e715d73`, 2026-11-16

## Why this matters

Sawit's terminal has **Copy Last Output / Copy Error**, and AI agents read the same text through the
MCP tool `terminal_read` with `output: "last"` or `"errors"`. While a command is still running (a dev
server, a watcher, a test run — exactly when an agent wants its output), the app asks for "from the
command to the end" by passing `usize::MAX`. `text_from` converts marks with `as i32`, and
`usize::MAX as i32` is `-1`, which clamps to the *top* of the scrollback, so the range is empty and
the user/agent gets "Nothing to copy" / an empty string.

## Current state

- `crates/app/src/terminal/view/copy_out.rs:13-27`:
  ```rust
  /// The output of the last command typed at the prompt: from the row after it to the prompt
  /// that followed (or to the end while it still runs); the screen when no command was seen.
  pub fn last_output(&self) -> String {
      match self.command_mark.get() {
          Some(mark) => {
              let end = if self.at_prompt() {
                  self.session.cursor_mark()
              } else {
                  usize::MAX
              };
              self.session.text_from(mark + 1, end)
          }
          None => self.session.screen_text(),
      }
  }
  ```
- `crates/core/src/terminal/session/text.rs:118-140`:
  ```rust
  pub fn cursor_mark(&self) -> usize {
      let term = self.term.lock();
      let grid = term.grid();
      grid.history_size() + grid.cursor.point.line.0.max(0) as usize
  }

  pub fn text_from(&self, from: usize, to: usize) -> String {
      let term = self.term.lock();
      let grid = term.grid();
      let history = grid.history_size() as i32;
      let last = grid.screen_lines() as i32 - 1;
      let row_of = |mark: usize| (mark as i32 - history).clamp(-history, last + 1);
      let (start, end) = (row_of(from), row_of(to));
      if start >= end {
          return String::new();
      }
      …
  ```
  Rows run from `-history` (oldest scrollback) to `last` (bottom screen row); `last + 1` means
  "through the end of the screen".
- Existing test to copy: `crates/app/src/terminal/view/copy_out.rs:59-91`
  `the_last_commands_output_is_copied_without_codes`, a `#[gpui_kit::test]` using
  `crate::terminal::testing::{init, open_panel, ready, wait_until}`:
  ```rust
  cx.update(init);
  let (_, panel) = open_panel(cx);
  let view = cx.update(|cx| panel.read(cx).tab(0).read(cx).focused_view().clone());
  ready(cx, &view);
  let command = "printf 'one\\n\\033[31merror: two\\033[0m\\nthree\\n'";
  cx.update(|cx| view.read(cx).write(sawit_core::terminal::run_bytes(command)));
  wait_until(cx, "the command ran", |cx| {
      let v = view.read(cx);
      v.at_prompt() && v.text_tail(20).contains("three")
  });
  let out = cx.update(|cx| view.read(cx).last_output());
  assert_eq!(out, "one\nerror: two\nthree");
  ```
  copy_out.rs is 92 lines.

## Commands you will need

**Environment (mandatory for every cargo command):** prefix with
```
source /Volumes/bahrul/.toolchains/env.sh && export CARGO_TARGET_DIR=/Volumes/bahrul/project/sawit-advisor-target &&
```
Run cargo in the Sawit app's terminal: `sawit` MCP tool `terminal_run` (ToolSearch
`select:mcp__sawit__terminal_run`), `new_tab: true`, `cwd` = worktree root, command ending in
`2>&1 | tee /Volumes/bahrul/.toolchains/logs/advisor-004.log; echo "exit=${pipestatus[1]}" > /Volumes/bahrul/.toolchains/logs/advisor-004.done`.
Wait for the `.done` file (Monitor tool, or Bash `test -f` every minute), read the log, delete the
`.done` before reuse. Never type into a tab you didn't just create. Shared target dir: lock waits
and long first builds are normal.

| Purpose | Command (after env prefix) | Expected |
|---|---|---|
| App test | `cargo test -p sawit --bin sawit terminal::view::copy_out` | all pass |
| Core tests | `cargo test -p sawit-core --lib terminal::session` | all pass |
| Clippy | `cargo clippy -p sawit-core -p sawit --all-targets -- -D warnings` | exit 0 |
| Line limit | `sh scripts/check-line-limit.sh crates/core/src/terminal crates/app/src/terminal` | all ≤ 500 |

## Scope

**In scope**: `crates/core/src/terminal/session/text.rs`, `crates/app/src/terminal/view/copy_out.rs`.

**Out of scope**: marks going stale once the 10,000-line scrollback is full (a separate, larger
issue — don't attempt it); `crates/app/src/terminal/panel/control.rs` (it just calls `last_output`).

## Git workflow

Commit on your worktree's branch, e.g.
`fix(terminal): Copy Last Output and terminal_read's last output work while the command still runs`.
Do NOT push.

## Steps

### Step 1: Failing test

In copy_out.rs's `mod tests`, add a `#[gpui_kit::test]` (doc comment in the file's style) that runs
`printf 'one\\ntwo\\n'; sleep 30`, waits until `!v.at_prompt() && v.text_tail(20).contains("two")`,
and asserts `last_output()` is `"one\ntwo"`. Before the test ends, interrupt the sleep so the shell
exits cleanly: `view.read(cx).write(b"\x03")` (Ctrl-C) — check the view's write method accepts
bytes as in the existing test; adapt to its real signature.

**Verify**: app test → the new test FAILS (empty string), the old one passes.

### Step 2: Fix the row conversion

In `text_from`, compute rows without truncation, e.g.
```rust
let history = grid.history_size() as i64;
let last = grid.screen_lines() as i64 - 1;
let row_of = |mark: usize| {
    (i64::try_from(mark).unwrap_or(i64::MAX).saturating_sub(history)).clamp(-history, last + 1) as i32
};
```
so `usize::MAX` means "through the end". Keep the loop's types consistent (it indexes
`grid[Line(row)]` with `i32`). Update the doc comment of `text_from` to say a `to` past the screen
reads to its end. Leave `copy_out.rs`'s `usize::MAX` call as is (it is now correct), unless you
prefer a named constant — optional.

**Verify**: app test → both tests pass; core tests → pass.

### Step 3: Lint and limits

**Verify**: clippy → exit 0; line limit → passes. Only rustfmt files that were clean before.

## Test plan

New app test: output of a still-running command. Existing test: output after the prompt returns.

## Done criteria

- [ ] `cargo test -p sawit --bin sawit terminal::view::copy_out` → 2+ tests pass
- [ ] `grep -n "mark as i32" crates/core/src/terminal/session/text.rs` → no match
- [ ] clippy exit 0, line limit passes
- [ ] `git status` shows only the two in-scope files

## STOP conditions

- Excerpts don't match the live code.
- The new test still fails after Step 2 for a reason other than the row math (e.g. `at_prompt()`
  is true while `sleep` runs) — report what you observed.
- The test is flaky (passes/fails across 3 runs).

## Maintenance notes

The deferred bug: once scrollback reaches its cap (10,000 lines, `session/spawn.rs`), marks stored
as `history_size + line` no longer move with the content; fixing it needs a monotonic line counter.
