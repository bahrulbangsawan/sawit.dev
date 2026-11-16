# Plan 006: Agent tools can't overwrite files without `overwrite`, MySQL's read-only splitter always agrees with the server, and Keychain names with control characters are refused

> **Executor instructions**: Follow this plan step by step. Run every
> verification command and confirm the expected result before moving to the
> next step. If anything in the "STOP conditions" section occurs, stop and
> report — do not improvise. When done, update the status row for this plan
> in `plans/README.md` — unless a reviewer dispatched you and told you they
> maintain the index.
>
> **Drift check (run first)**: `git diff --stat 7e715d73..HEAD -- crates/mcp/src/paths.rs crates/mcp/src/lib.rs crates/mcp/src/app/canvas crates/mcp/src/diagrams.rs crates/mcp/src/browser crates/core/src/db/mysql crates/core/src/keychain.rs`
> If any in-scope file changed since this plan was written, compare the
> "Current state" excerpts against the live code before proceeding; on a
> mismatch, treat it as a STOP condition.

## Status

- **Priority**: P2
- **Effort**: M (three independent S parts)
- **Risk**: LOW
- **Depends on**: none
- **Category**: security
- **Planned at**: commit `7e715d73`, 2026-11-16

## Why this matters

Sawit's MCP server (`crates/mcp`, binary `sawit-mcp`) lets AI agents drive the app. Agents act on
text they read (web pages, repos), so tools must not do more than they say. Three gaps:

- **A. Silent overwrites.** Tools that write a file to an agent-chosen `output_path`
  (`canvas_export`, `canvas_screen_export`, `render_diagram`/`render_mermaid`, `schema_diagram`,
  `browser_screenshot`) overwrite whatever is there — e.g. `~/.zshrc` with SVG bytes. The database
  export tools already refuse an existing file unless `overwrite: true`; the others must match.
- **B. MySQL read-only can be bypassed.** The read-only path (`query` tool, "always read-only")
  splits SQL into statements on the client, checks each one's first keyword, then sends the
  **whole text** to the server in one call. The client splitter treats `\` as an escape inside
  quotes and `"` as a string quote. A server whose `sql_mode` has `NO_BACKSLASH_ESCAPES` or
  `ANSI_QUOTES` splits differently, so it can see (and run) a statement the client never checked,
  such as one that ends the read-only transaction and writes. Fix: on read-only connections, set the
  session `sql_mode` without those two flags, so the server lexes like the client.
- **C. Keychain command injection.** `keychain::set` writes one command line to
  `/usr/bin/security -i` (interactive mode reads one command per line). Its quoting escapes `"` and
  `\` but not newlines, and the account name is built from connection fields that agents can
  supply. A newline in a name would start a second `security` command. Refuse control characters.

## Current state

### A — output paths (`crates/mcp`)

- `crates/mcp/src/paths.rs:31-47`:
  ```rust
  pub(crate) fn output_path(tool: &str, output_path: Option<&str>) -> Result<Option<PathBuf>, String> {
      let Some(p) = output_path.map(str::trim).filter(|p| !p.is_empty()) else {
          return Ok(None);
      };
      let path = expand_home(p);
      if !path.is_absolute() {
          return Err(format!("output_path “{p}” must be absolute."));
      }
      refuse_canvas_file(tool, &path)?;
      Ok(Some(path))
  }
  ```
  (re-exported so callers write `crate::output_path`; paths.rs is 237 lines.)
- The model to match — `crates/mcp/src/db_admin/mod.rs:130-150` `output_file(tool, path, overwrite)`:
  ```rust
  if file.exists() && !overwrite {
      return Err(format!(
          "{} already exists. Pass overwrite=true to replace it.",
          file.display()
      ));
  }
  ```
  and its args field (look at `ExportTableArgs` in the same file for the `overwrite` field and its
  `#[schemars(description = …)]`; copy that wording).
- Callers that write **one file at the named path** (change these):
  - `crates/mcp/src/app/canvas/export.rs:27` (`canvas_export`, args `CanvasExportArgs` in `crates/mcp/src/app/canvas/args.rs:171`, field `output_path`)
  - `crates/mcp/src/app/canvas/screens.rs:191` and `:232` (`canvas_screen_export`, args `CanvasScreenExportArgs` at `args.rs:332`, field `path`)
  - `crates/mcp/src/diagrams.rs:141` (`render_diagram_tool`, args `RenderArgs` at `diagrams.rs:36`) and `:149` (`schema_diagram`, `SchemaDiagramArgs` at `diagrams.rs:54`)
  - `crates/mcp/src/browser/mod.rs:237` (`browser_screenshot` single shot, `ScreenshotArgs` at `crates/mcp/src/browser/args.rs:150`)
- Callers to **leave alone** (they name a folder/stem or a file written later by the app):
  `crates/mcp/src/browser/capture.rs:14` (recording) and `:40` (`sized_screenshot`).
- Existing test using `output_path`: `crates/mcp/src/app/canvas/tests/export.rs:189`
  `an_export_path_is_read_as_every_tool_reads_one`.
- `args.rs` is 446 lines; `browser/args.rs` 405; `browser/mod.rs` 439; limit is 500 per file.

### B — MySQL read-only session (`crates/core/src/db/mysql`)

- `crates/core/src/db/mysql/mod.rs:136-150`:
  ```rust
  fn opts(profile: &ConnProfile, password: Option<&str>, read_only: bool, tls: Option<SslOpts>) -> Opts {
      let mut setup: Vec<String> = Vec::new();
      if read_only {
          setup.push("SET SESSION TRANSACTION READ ONLY".into());
      }
      let mut b = OptsBuilder::default()
          …
          .setup(setup)
  ```
  `setup` statements run on each pooled connection (the existing READ ONLY setting relies on it).
- `crates/core/src/db/mysql/run.rs:221-237` (`run_if_read_only`): `split_statements(sql)` →
  `write_reason` per statement → screens the text `transaction_read_only`/`tx_read_only` →
  `START TRANSACTION READ ONLY` → `run_on(&mut conn, sql, …)` with the **whole** `sql`.
  `write_reason` (run.rs:117) allows only `select|with|show|describe|desc|explain|table|values|help`,
  so a visible `SET …` is already refused.
- `literal()` in mod.rs already writes values so they read the same with or without
  `NO_BACKSLASH_ESCAPES` (see `mysql/tests.rs` `quotes_names_and_values`).
- Tests without a server: `crates/core/src/db/mysql/tests.rs` (356 lines; `use super::*;`).
  Live tests: `crates/core/src/db/mysql/live_tests.rs` (160 lines), run only with
  `SAWIT_TEST_MYSQL_URL=mysql://root@127.0.0.1:3307/` and skip otherwise (`let Some(..) = live() else { return; };`).

### C — Keychain (`crates/core/src/keychain.rs`, 216 lines)

- `set` (line ~120): deletes the old item, then sends
  `format!("add-generic-password -s {} -a {} -j {} {data}\n", quote(service), quote(account), quote(MARKER))`
  to `security -i` on stdin. `quote` (line ~166):
  ```rust
  /// A word for `security -i`, which splits on spaces and honours `"` and `\`.
  fn quote(s: &str) -> String {
      format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
  }
  ```
- `delete`, `exists`, reads pass names as separate argv entries (safe).
- Tests at the bottom: `mod tests { #[test] fn hex_round_trips() …  #[test] fn quote_escapes() … }`.

## Commands you will need

**Environment (mandatory for every cargo command):** prefix with
```
source /Volumes/bahrul/.toolchains/env.sh && export CARGO_TARGET_DIR=/Volumes/bahrul/project/sawit-advisor-target &&
```
Run cargo in the Sawit app's terminal: `sawit` MCP tool `terminal_run` (ToolSearch
`select:mcp__sawit__terminal_run`), `new_tab: true`, `cwd` = worktree root, command ending in
`2>&1 | tee /Volumes/bahrul/.toolchains/logs/advisor-006.log; echo "exit=${pipestatus[1]}" > /Volumes/bahrul/.toolchains/logs/advisor-006.done`.
Wait for the `.done` file (Monitor tool, or Bash `test -f` every minute), read the log, delete the
`.done` before reuse. Never type into a tab you didn't just create. Shared target dir: lock waits
and long first builds are normal.

| Purpose | Command (after env prefix) | Expected |
|---|---|---|
| MCP tests | `cargo test -p sawit-mcp --lib` | all pass |
| Core tests | `cargo test -p sawit-core --lib db::mysql keychain` | all pass |
| MySQL live (optional) | `SAWIT_TEST_MYSQL_URL=mysql://root@127.0.0.1:3307/ cargo test -p sawit-core --lib db::mysql` | all pass |
| Clippy | `cargo clippy -p sawit-core -p sawit-mcp --all-targets -- -D warnings` | exit 0 |
| Line limit | `sh scripts/check-line-limit.sh crates/core crates/mcp` | all ≤ 500 |

Whether a MySQL server is up: `lsof -nP -iTCP:3307 -sTCP:LISTEN`. Do **not** start or install a
MySQL server; if none is listening, skip the live run and say so.

## Scope

**In scope**: `crates/mcp/src/paths.rs`, `crates/mcp/src/lib.rs` (only if the re-export needs it),
`crates/mcp/src/app/canvas/{args.rs,export.rs,screens.rs}`, `crates/mcp/src/app/canvas/tests/export.rs`,
`crates/mcp/src/diagrams.rs`, `crates/mcp/src/browser/{args.rs,mod.rs}`,
`crates/core/src/db/mysql/{mod.rs,run.rs,tests.rs,live_tests.rs}`, `crates/core/src/keychain.rs`.

**Out of scope**: `browser/capture.rs` (recording / multi-size screenshots); the Postgres `query`
path; `crates/app/**`; the `skills/` docs; changing what any tool writes by default when no path is given.

## Git workflow

Commit each part separately on your worktree's branch, e.g.
`fix(mcp): canvas_export, canvas_screen_export, the diagram tools and browser_screenshot refuse to replace a file unless overwrite is true`,
`fix(databases): MySQL's read-only connections read SQL as the statement check does, whatever the server's sql_mode`,
`fix(keychain): names with control characters are refused`. Do NOT push.

## Steps

### Step 1 (A): `overwrite` for single-file writers

- In paths.rs add next to `output_path`:
  ```rust
  /// [`output_path`] for a tool that writes one file there: an existing file is refused unless
  /// `overwrite`, as the database exports refuse one.
  pub(crate) fn new_output_path(tool: &str, output_path: Option<&str>, overwrite: bool) -> Result<Option<PathBuf>, String>
  ```
  It calls `output_path`, then if `Some(path)` with `path.is_file() && !overwrite` returns the same
  message wording as `db_admin::output_file`. Export it the same way `output_path` is exported.
- Add `pub overwrite: Option<bool>` with a `#[schemars(description = …)]` (wording copied from the
  DB export args) to `CanvasExportArgs`, `CanvasScreenExportArgs`, `RenderArgs`,
  `SchemaDiagramArgs`, `ScreenshotArgs`. If any of these derive or are built with a struct literal
  elsewhere (tests), add `overwrite: None` there.
- Switch the six call sites listed in Current state from `crate::output_path(tool, x)` to
  `crate::new_output_path(tool, x, a.overwrite.unwrap_or(false))`.
- Tests (put them beside `an_export_path_is_read_as_every_tool_reads_one` in
  `app/canvas/tests/export.rs`, or in a `#[cfg(test)]` module in paths.rs): an existing file is
  refused without overwrite and accepted with it; a new file is accepted; `None` stays `None`.

**Verify**: MCP tests → pass; `grep -rn "crate::output_path(" crates/mcp/src | grep -v capture.rs | grep -v tests` → no match; line limit passes.

### Step 2 (B): pin `sql_mode` on read-only MySQL connections

In `opts`, when `read_only`, push a second setup statement **before** the READ ONLY one:
```sql
SET SESSION sql_mode = TRIM(BOTH ',' FROM REPLACE(REPLACE(CONCAT(',', @@SESSION.sql_mode, ','), ',NO_BACKSLASH_ESCAPES,', ','), ',ANSI_QUOTES,', ','))
```
with a comment: the statement check (`split_statements`, `write_reason`) reads `\` as an escape and
`"` as a string quote; with these two flags the server would read the same text as other
statements than the ones checked. Also extend the text screen in `run_if_read_only` to refuse
`read write` (squeezed whitespace, lowercase) alongside `transaction_read_only`.
- Unit test in `mysql/tests.rs`: `opts(&profile, None, true, None)` has the sql_mode statement in
  its setup and `opts(…, false, …)` doesn't (mysql_async's `Opts` exposes its setup list — find the
  accessor in the crate's docs/source under `$CARGO_HOME/registry/src`; if there is none, test a
  small helper `fn read_only_setup() -> Vec<String>` you factor out instead).
- Unit test: `run_if_read_only`'s screen refuses `SELECT 1; SET SESSION TRANSACTION READ WRITE` —
  only if it can be tested without a server; otherwise test the screening helper you extract.
- Live test in `live_tests.rs` (skips without the env var, like the others): set
  `SET GLOBAL sql_mode = CONCAT(@@GLOBAL.sql_mode, ',NO_BACKSLASH_ESCAPES,ANSI_QUOTES')` on a
  read-write connection, open a read-only connection, assert `SELECT @@SESSION.sql_mode` contains
  neither flag, then **restore** the global sql_mode to what it was (in all cases).

**Verify**: core tests → pass; live run if a server is listening on 3307 → pass.

### Step 3 (C): refuse control characters in Keychain names

Add `fn check_name(what: &str, s: &str) -> Result<()>` returning
`CoreError::Config(format!("Keychain: the {what} can't contain line breaks or control characters."))`
when `s.chars().any(char::is_control)`. Call it for `service` and `account` at the very top of
`set` (before `delete`) and of `delete`. Test: `set("svc", "a\nb", "x")` returns `Err` — this
must return before any `security` process runs, so the test never touches the real Keychain; and
`check_name` accepts `"user@host:5432/db"`.

**Verify**: core tests → pass.

### Step 4: Lint and limits

**Verify**: clippy → exit 0; line limit → passes. Only rustfmt files that were clean before.

## Test plan

- A: refused / accepted-with-overwrite / new-file / none cases.
- B: setup list contains the pin for read-only only; screen refuses `read write`; live sql_mode check (if a server is up).
- C: control characters refused before any `security` call; normal names accepted.

## Done criteria

- [ ] `cargo test -p sawit-mcp --lib` and `cargo test -p sawit-core --lib db::mysql keychain` pass, with ≥ 5 new tests
- [ ] `grep -n "NO_BACKSLASH_ESCAPES" crates/core/src/db/mysql/mod.rs` → a match in `opts`
- [ ] `grep -n "is_control" crates/core/src/keychain.rs` → a match
- [ ] clippy exit 0; line limit passes
- [ ] `git status` shows only in-scope files
- [ ] Report says whether the MySQL live test ran

## STOP conditions

- Excerpts don't match the live code.
- A `skills/` or README test fails because a tool's parameters changed (report it; don't edit docs).
- mysql_async rejects the `setup` statement form, or the live test shows the session still has
  `NO_BACKSLASH_ESCAPES`.
- Any test would write to the real login Keychain.

## Maintenance notes

- New MCP tools that write a file at an agent-chosen path should use `new_output_path` (or
  `db_admin::output_file`), never plain `output_path`.
- If the read-only MySQL path ever switches to the binary protocol (one statement per call), the
  sql_mode pin becomes defence in depth rather than the fix.
- Deferred: the Postgres `query` tool runs saved/detected targets under the profile's own login
  (often a superuser) inside a READ ONLY transaction; side-effecting server functions remain
  callable there. Needs its own plan (role per target, or reword "always read-only").
