# Browser tests 2: Run panel and report tab — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Browser tests in the app: a Tests section in the Run panel to run one test or all of them, a report tab per test that updates while it runs, and a "Fix with agent" button that hands a stuck test to Claude Code or Codex.

**Architecture:** A per-window `TestRuns` entity owns the run queue and runs `browser_test::runner::run_unattended` on the Tokio runtime with an `AppDriver` aimed at the window's project, receiving a report after every step. The Run panel's Tests section and each `TestReportView` observe it. All parsing, running and file layout come from plan 1; the app only lists, starts and shows.

**Tech Stack:** Rust 2024, GPUI via `gpui_kit`, `sawit_core::browser_test` (plan 1), `futures::channel::mpsc`, the app's `rt::spawn`.

**Spec:** `docs/superpowers/specs/2026-10-30-browser-tests-design.md` (section "In the app")

**Series:** 2 of 2. Needs plan 1 (`browser_test::{files::{list_tests, TestEntry, test_path, latest_report}, runner::run_unattended, report::{RunReport, RunOutcome, StepOutcome}, driver::AppDriver}`).

## Global Constraints

- The runner never runs on the UI thread: `rt::spawn`, results back through `cx.spawn`.
- One run at a time per project (plan 1's `RunLock`); a run refused because an agent holds the lock shows the lock's message in the report tab and on the row, and the queue stops.
- Outcome colours: passed / recorded → success colour; healed → warning colour; failed → danger colour; needs agent → warning colour; aborted, skipped, never run → muted. Use the theme's existing colour names (`cx.theme()`), as `run/panel.rs` and `ports.rs` do.
- Copy: "Tests" (section), "Run all tests", "Run again", "Fix with agent", "Open test file", "Copy summary", "Never run", "{recorded}/{steps} recorded", and the agent prompt verbatim from the spec.
- Commits: `feat(browser-test): …`, ending with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.

## Review Focus

1. **A test file edited while its report tab is open**: the tab's step list comes from the report being shown, not the file, so it never indexes past the steps it has; the next run shows the new steps — test in Task 2 (`step_rows` with a report shorter than the spec).
2. **Run all while one test is already running from an agent**: the first run fails with the lock message; the queue stops instead of failing every test in turn — test in Task 1 (`queue_stops_on_lock_error`).
3. **The project folder changes (another folder opened in the window) during a run**: the run finishes against the folder it started with; the Tests section reloads for the new folder and doesn't show the old run as its own — test in Task 1 (`runs_keep_their_project`).
4. **No dev server running**: Run shows "Start the dev server or give a base URL." on the row and in the tab, and no browser tab opens — covered by plan 1's `base_url` error; checked by hand in Task 4.
5. **Neither `claude` nor `codex` on PATH, or Sawit's MCP server missing from it**: "Fix with agent" opens the MCP page instead of typing a command that can't work — test in Task 3 (`agent_command_choice`).

---

## File Structure

| File | Responsibility |
|---|---|
| `crates/app/src/browser_tests.rs` (new) | `TestRuns` entity: queue, live reports, events; `fix_prompt`, `agent_command` |
| `crates/app/src/run/tests.rs` (new) | Tests section of the Run panel |
| `crates/app/src/run/panel.rs` | render the Tests section; new `RunEvent` variants |
| `crates/app/src/run/mod.rs` | `mod tests;` |
| `crates/app/src/test_report.rs` (new) | `TestReportView`, the report tab |
| `crates/app/src/workspace.rs` | own `TestRuns`; handle the new `RunEvent`s; `open_test_report` |
| `crates/app/src/main.rs` | `mod browser_tests; mod test_report;` |

---

### Task 1: `TestRuns`

**Files:**
- Create: `crates/app/src/browser_tests.rs`
- Modify: `crates/app/src/main.rs`, `crates/app/src/workspace.rs` (create one `TestRuns` per workspace, `set_root` it when the project changes)
- Test: `crates/app/src/browser_tests.rs` (`mod tests`: plain unit tests for `next_after`; `#[gpui::test]` with `TestAppContext`, as `terminal/panel.rs` does, for the entity)

**Interfaces:**
- Consumes: `files::{list_tests, TestEntry, latest_report}`, `runner::run_unattended`, `driver::AppDriver`, `report::RunReport`.
- Produces:

```rust
pub enum TestRunsEvent {
    /// A test's live or final report changed.
    Report(String),
    /// The list of tests was reloaded.
    Listed,
}
pub struct TestRuns { /* root, entries, queue: VecDeque<String>, running: Option<(PathBuf, String)>, live: HashMap<String, RunReport>, errors: HashMap<String, String> */ }
impl EventEmitter<TestRunsEvent> for TestRuns {}
impl TestRuns {
    pub fn new(root: Option<PathBuf>, cx: &mut Context<Self>) -> Self; // lists at once (off the UI thread)
    pub fn set_root(&mut self, root: Option<PathBuf>, cx: &mut Context<Self>);
    pub fn reload(&mut self, cx: &mut Context<Self>);
    pub fn entries(&self) -> &[TestEntry];
    /// Queues these tests (skipping ones already queued or running) and starts the first.
    pub fn run(&mut self, names: Vec<String>, cx: &mut Context<Self>);
    pub fn is_running(&self, name: &str) -> bool;
    pub fn is_queued(&self, name: &str) -> bool;
    /// The live report while running, else the newest on disk.
    pub fn report(&self, name: &str) -> Option<RunReport>;
    /// A run that couldn't start (lock, base URL, parse): shown on the row and in the tab.
    pub fn error(&self, name: &str) -> Option<&str>;
}
/// Pure queue step: what to start next, given the previous run's result.
fn next_after(queue: &mut VecDeque<String>, previous: &Result<RunReport, String>) -> Option<String>;
```

  A run: `rt::spawn(run_unattended(&AppDriver { paths: AppPaths::default_macos(), project: Some(root) }, &root, &name, None, Some(tx)))`; a `cx.spawn` loop reads `rx` into `live` and emits `Report(name)`; at the end it stores the error (if any), reloads the list, and starts `next_after`. A result `Err` whose text contains `already running` clears the queue.

- [ ] **Step 1: Write the failing tests:**
  - `queue_runs_in_order`: `next_after` with queue `[b, c]` and `Ok(report)` → `Some(b)`, then `Some(c)`, then `None`.
  - `queue_stops_on_lock_error`: queue `[b, c]`, previous `Err("A browser test is already running (pid 7).")` → `None` and the queue is empty.
  - `other_errors_keep_going`: previous `Err("Start the dev server or give a base URL.")` → `Some(b)`.
  - `runs_keep_their_project` (`TestAppContext`, two temp project folders, the run's spawned future never polled to completion): after `run(["a"])` with root `/p1` then `set_root(/p2)`, `running` still holds `/p1`, and `entries()` reloads for `/p2`.
- [ ] **Step 2: Run** `cargo test -p sawit browser_tests` — Expected: FAIL to compile.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** — Expected: PASS.
- [ ] **Step 5: Commit** `feat(browser-test): test runs in the app`.

---

### Task 2: The report tab

**Files:**
- Create: `crates/app/src/test_report.rs`
- Modify: `crates/app/src/workspace.rs` (`open_test_report(name, window, cx)`, modelled on `open_changelog_page`: `PaneItem::Other` with key `test:<name>`, the test's title, icon `Lucide::CircleCheck`, group = the project root)
- Test: `crates/app/src/test_report.rs` (`mod tests`)

**Interfaces:**
- Consumes: `TestRuns` (Task 1); `RunReport`, `StepReport`, `StepOutcome::label`, `RunReport::summary_markdown`.
- Produces:

```rust
pub enum TestReportEvent { RunAgain(String), FixWithAgent(String), OpenFile(PathBuf) }
pub struct TestReportView { name: String, runs: Entity<TestRuns>, root: PathBuf, expanded: HashSet<usize>, _sub: Subscription }
impl EventEmitter<TestReportEvent> for TestReportView {}
impl TestReportView { pub fn new(name: String, root: PathBuf, runs: Entity<TestRuns>, cx: &mut Context<Self>) -> Self; }
/// What each card shows: from the report, never indexing the spec.
struct StepRow { index: usize, text: String, outcome: StepOutcome, duration: String, error: Option<String>, checks: Vec<(bool, String)>, console: Vec<String>, requests: Vec<String>, screenshot: Option<PathBuf> }
fn step_rows(report: &RunReport, run_dir: &Path) -> Vec<StepRow>;
fn format_duration(ms: u64) -> String; // "850 ms", "4.2 s", "1 m 05 s"
```

  Layout (top to bottom): title; outcome badge, base URL, start time and duration; warnings; buttons Run again (disabled while queued/running), Fix with agent (only for needs agent / failed), Open test file, Copy summary (writes `summary_markdown()` to the clipboard); healed steps get a "healed" badge and sort first only in the summary, not the cards; one card per `StepRow` with checks ✓/✗, collapsible console errors and failed requests ("none" when empty, collapsed), and the screenshot as a thumbnail (`img(path)` sized 240 px wide) that emits `OpenFile` for Sawit's image viewer. With no report and no error: "Never run" and a Run button. The view re-renders on `TestRunsEvent::Report(name)` for its own name.

- [ ] **Step 1: Write the failing tests:**
  - `rows_come_from_the_report`: a report with 2 steps (one failed with a check `text "1 item"` ✗ and one console error, one skipped) → two rows; row 1 `checks == [(false, "text \"1 item\"")]`, `console` has the error text; `screenshot` joins the run dir and file name.
  - `rows_ignore_spec_length`: a 2-step report for a test whose file now has 4 steps → 2 rows.
  - `durations`: `format_duration(850)` == `850 ms`, `4200` → `4.2 s`, `65000` → `1 m 05 s`.
- [ ] **Step 2: Run** `cargo test -p sawit test_report` — Expected: FAIL to compile.
- [ ] **Step 3: Implement** the view and `open_test_report`; the workspace handles `RunAgain` (`runs.run(vec![name])`), `OpenFile` (`open_file`) and, in Task 3, `FixWithAgent`.
- [ ] **Step 4: Run** — Expected: PASS.
- [ ] **Step 5: Commit** `feat(browser-test): report tab`.

---

### Task 3: Tests section and Fix with agent

**Files:**
- Create: `crates/app/src/run/tests.rs`
- Modify: `crates/app/src/run/panel.rs` (hold `Entity<TestRuns>`; render the section after the script groups with `render_section("run-section-tests", "Tests", …)`; observe `TestRuns`), `crates/app/src/run/mod.rs`, `crates/app/src/workspace.rs` (handle new events), `crates/app/src/browser_tests.rs` (agent helpers)
- Test: `crates/app/src/browser_tests.rs`, `crates/app/src/run/tests.rs`

**Interfaces:**
- Consumes: `TestRuns` (Task 1), `open_test_report` (Task 2), `Workspace::run_in_terminal`, the CLI detection in `mcp_page.rs` (`find_in_path` for `claude` and `codex`) and `mcp_clients::{discover, status, Spec::sawit}` for whether Sawit's server is installed in that client.
- Produces:

```rust
// run/panel.rs
pub enum RunEvent { Start(Vec<Launch>), Show(String), RunTests(Vec<String>), OpenTestReport(String) }

// run/tests.rs
/// One row: status, title, "{recorded}/{steps} recorded" or the parse error.
struct TestRow { name: String, title: String, detail: String, status: RowStatus, runnable: bool }
#[derive(Debug, Clone, Copy, PartialEq)]
enum RowStatus { Running, Queued, Passed, Healed, Failed, NeedsAgent, Aborted, NeverRun, Broken }
fn test_rows(entries: &[TestEntry], runs: &TestRuns) -> Vec<TestRow>;

// browser_tests.rs
pub fn fix_prompt(name: &str) -> String;
/// "claude" when its CLI is found and Sawit's server is installed in Claude Code, else "codex" under
/// the same rule, else None (the caller opens the MCP page).
pub fn agent_command(found: &[(&'static str, bool)], name: &str) -> Option<String>;
```

  `fix_prompt("checkout")` returns exactly: `Run the Sawit browser test "checkout" with browser_test_run. When a step needs you, do it with the browser_* tools and call browser_test_step_done. If the app is what's broken, call it with outcome bug and a note saying what is wrong.` `agent_command` returns `{cli} {quoted prompt}` with the prompt single-quoted for the shell (`'` → `'\''`). A row's status: `Running`/`Queued` from `TestRuns`, else the last report's outcome (a run whose steps include `Healed` and that passed → `Healed`), else `NeverRun`; `Broken` when the spec doesn't parse (not runnable; detail is `line N: …`). The section header's play button ("Run all tests") emits `RunTests` with every runnable name; a row's play button emits `RunTests([name])` and `OpenTestReport(name)`; clicking a row emits `OpenTestReport(name)`; the panel's existing refresh also calls `runs.reload`. No section when the project has no `.sawit/tests` folder. "Fix with agent" in the workspace: `agent_command` → `run_in_terminal` in the project root, else `open_page(Page::Mcp)`.

- [ ] **Step 1: Write the failing tests:**
  - `prompt_is_the_spec_text`: `fix_prompt("checkout")` equals the sentence above.
  - `agent_command_choice`: `[("claude", true), ("codex", true)]` → starts with `claude '`; `[("claude", false), ("codex", true)]` → starts with `codex '`; `[("claude", false)]` → `None`; `[]` → `None`; a name containing `'` is escaped.
  - `rows_show_status_and_progress`: entries `a` (2/3 recorded, last report passed with a healed step), `b` (parse error on line 4), `c` (never run) with `a` queued → `a` is `Queued` (queue wins over history), `b` is `Broken` with detail `line 4: …` and `runnable == false`, `c` is `NeverRun` with detail `0/… recorded`.
- [ ] **Step 2: Run** `cargo test -p sawit browser_tests run::tests` — Expected: FAIL to compile.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `cargo test -p sawit && cargo clippy --workspace --all-targets -- -D warnings` — Expected: PASS, no warnings.
- [ ] **Step 5: Commit** `feat(browser-test): Tests section in the Run panel and Fix with agent`.

---

### Task 4: Check it in the app

**Files:** none new (uses plan 1's `docs/fixtures/browser/shop.html` and `.sawit/tests/shop.md`).

- [ ] **Step 1:** Without a server running, open `docs/fixtures/browser` in Sawit (`cargo run -p sawit`), Run panel → Tests shows `shop` as "Never run"; press its play button — Expected: the report tab opens showing "Start the dev server or give a base URL."; no browser tab opens.
- [ ] **Step 2:** Add a saved Run command `python3 -m http.server 8765` in that folder and start it (so `pick_dev_server` finds it). Run `shop` with no recording — Expected: browser tab comes forward, step 1 ends *needs agent*, steps 2–3 *skipped*, row shows the warning colour; "Fix with agent" is shown.
- [ ] **Step 3:** Press "Fix with agent" — Expected: a terminal tab in the project runs `claude '…the prompt…'`; let the agent finish; the row and tab update to passed with steps *recorded* after the next reload.
- [ ] **Step 4:** Press "Run again" — Expected: all steps *passed*, the cards fill in one by one while it runs, thumbnails open in the image viewer, "Copy summary" puts the Markdown on the clipboard.
- [ ] **Step 5:** Make a second test file with a bad expectation line — Expected: its row shows `line N: …` and no play button; "Run all tests" runs only `shop`.
- [ ] **Step 6:** Start `browser_test_run name=shop` from an agent and leave a step waiting; press Run in the app — Expected: "A browser test is already running (pid N)." on the row and in the tab.
- [ ] **Step 7:** Remove `.runs/` and `.env`, leave the fixture as committed by plan 1. No commit unless a fix was needed.
