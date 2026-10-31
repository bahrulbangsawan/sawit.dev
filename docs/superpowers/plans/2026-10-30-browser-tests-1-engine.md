# Browser tests 1: engine and agent tools — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Plain-language tests in `.sawit/tests/*.md` that an agent runs through `sawit-mcp`: Sawit replays recorded steps itself, hands a step to the agent only when it is new or stale, records what the agent did, and writes a report.

**Architecture:** A new `sawit_core::browser_test` module holds the test format, recordings, a `Driver` trait over browser ops, the replay `Runner`, reports, a run lock and file helpers. The page agent learns stable locators. `sawit-mcp` gets three `browser_test_*` tools; a step waiting for the agent is held in the MCP process, and the existing `browser_*` tools record into it.

**Tech Stack:** Rust 2024, serde/serde_json, tokio (time, `spawn_blocking`), futures `mpsc`, `libc` (pid check), rmcp, the existing WKWebView page agent (`browser_agent.js`). No new dependencies.

**Spec:** `docs/superpowers/specs/2026-10-30-browser-tests-design.md`

**Series:** 1 of 2. Plan 2 (`2026-10-30-browser-tests-2-app.md`) adds the Run panel section and report tab on top of `browser_test::{files, runner, report}`.

## Global Constraints

- Tests: `<project>/.sawit/tests/<name>.md`; recordings `<name>.steps.json` beside it; runs `<project>/.sawit/tests/.runs/<name>/<unix-ms>/` with `report.json` and `step-NN.png` (01-based, two digits); keep the newest **10** runs per test; write `.sawit/tests/.gitignore` = `.runs/\n.env\n` when missing.
- Variables `{{NAME}}` come from `.sawit/tests/.env` first, then the environment.
- Replay timeouts: each action retried and each expectation polled for up to **5 s** (`STEP_TIMEOUT`); loads wait up to 15 s; after a click, key press or submit, wait 250 ms then for the load (as `browser_*` does today).
- Locator replay order: test id (`data-testid`, `data-test`, `data-cy`, `data-qa`) → role + name + `nth` (visible elements, document order, through open shadow roots) → `#id`. `#id` is only recorded when unique and not generated: no run of 3+ digits, not React `:r…:`, not 16+ hex/dash chars.
- Recording file: pretty JSON, `"version": 1`, keys sorted, trailing newline.
- One run at a time per project, across processes (`.sawit/tests/.runs/lock` holding the pid).
- Sawit never calls a model.
- Copy uses the exact strings in the spec (error messages, the agent prompt, step outcomes).
- Commits: `feat(browser-test): …` (or `refactor(browser): …` for Task 4), ending with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.

## Review Focus

1. **Two elements with the same role and name** (two "Add to cart" buttons): the recording stores `nth`, and replay clicks the same one, not the first — test in Task 1 (wire) and Task 8 (fixture with two "Add" buttons).
2. **A secret typed during a heal**: a value equal to a variable is stored as `{{NAME}}`; a password that matches none is stored and the report warns naming the file — tests in Task 3 and Task 6.
3. **The MCP server exits while a step waits for the agent**: the lock file keeps a dead pid; the next run (app or agent) takes the lock over instead of failing forever — test in Task 5.
4. **Editing one step in the file**: only that step's recording is dropped; the others still replay without the agent — test in Task 2 (keys) and Task 3 (`prune`).
5. **The agent calls `browser_test_step_done outcome=done` but the check still fails**: the step stays open, the reply names the failing check, and the actions recorded so far are kept for the next attempt — test in Task 6.

---

## File Structure

| File | Responsibility |
|---|---|
| `crates/core/src/browser.rs` | `Locator`; `Target.locator`; `Query.locator`; `base64_decode` (moved from `sawit-mcp`) |
| `crates/core/src/browser_agent.js` | `locatorOf`, locator resolution, `secret` on password fields |
| `crates/core/src/app_control.rs` | `REPLY_TIMEOUT` (moved from `sawit-mcp/src/app.rs`) |
| `crates/core/src/browser_test/mod.rs` (new) | module root, re-exports |
| `crates/core/src/browser_test/spec.rs` (new) | test file parser, `Expect`, step keys, variables |
| `crates/core/src/browser_test/record.rs` (new) | `Action`, `Recording`, `action_from`, URL helpers |
| `crates/core/src/browser_test/driver.rs` (new) | `Driver`, `AppDriver`, load/wait polling |
| `crates/core/src/browser_test/report.rs` (new) | `RunReport`, outcomes, Markdown summary |
| `crates/core/src/browser_test/files.rs` (new) | paths, listing, `.env`, run dirs, `.gitignore`, base URL |
| `crates/core/src/browser_test/lock.rs` (new) | `RunLock` |
| `crates/core/src/browser_test/runner.rs` (new) | `Runner`, `prepare`, `run_unattended` |
| `crates/core/src/browser_test/fake.rs` (new, `#[cfg(test)]`) | `FakeDriver` for runner and driver tests |
| `crates/mcp/src/browser.rs` | use `driver` polling; record actions |
| `crates/mcp/src/browser_test.rs` (new) | `browser_test_list`, `browser_test_run`, `browser_test_step_done`, paused run |
| `crates/mcp/src/lib.rs` | `tests` state on `Sawit`, router, instructions, tool-name test |
| `docs/fixtures/browser/shop.html`, `docs/fixtures/browser/.sawit/tests/shop.md` (new) | manual end-to-end check |

---

### Task 1: Locators in the page agent

**Files:**
- Modify: `crates/core/src/browser.rs`, `crates/core/src/browser_agent.js`, `crates/mcp/src/browser.rs` (`target()` builds with `..Default::default()`), `crates/mcp/src/app.rs` (remove `base64_decode`, import from core)
- Test: `crates/core/src/browser.rs` (`mod tests`)

**Interfaces:**
- Produces:

```rust
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Locator {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub testid: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Which match, in page order, among visible elements with this role and name.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub nth: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub css: Option<String>,
}
impl Locator {
    pub fn is_empty(&self) -> bool;
    /// For messages: `button “Pay”` (with ` #2` when nth > 0), else `[data-testid=x]`, else the css.
    pub fn describe(&self) -> String;
}
// Target gains (serde key "locator", skipped when None); is_empty() also checks it:
pub locator: Option<Locator>,
// BrowserOp::Query gains:
#[serde(default, skip_serializing_if = "Option::is_none")] locator: Option<Locator>,
pub fn base64_decode(text: &str) -> Result<Vec<u8>, String>; // moved verbatim from sawit-mcp
```

  Page agent answers: `click`, `hover`, `type`, `select`, `scroll` (when it had an element) add `"locator": locatorOf(el)`; `type` adds `"secret": true` when the field is `input[type=password]`.

- [ ] **Step 1: Write the failing tests** in `browser.rs`:
  - `locator_wire_format`: `serde_json::from_str::<BrowserOp>(r#"{"op":"click","locator":{"role":"button","name":"Add","nth":1}}"#)` equals `Click { target: Target { locator: Some(Locator { role: Some("button".into()), name: Some("Add".into()), nth: 1, ..Default::default() }), ..Default::default() }, double: false, button: None }`; serializing `Locator { role: Some("link".into()), name: Some("Docs".into()), ..Default::default() }` gives exactly `{"role":"link","name":"Docs"}` (no `nth`); `Target { locator: Some(Locator::default()), ..Default::default() }.is_empty()` is `false`.
  - `locator_descriptions`: role+name → `button “Pay”`; nth 2 → `button “Pay” #3`; testid only → `[data-testid=cart]`; css only → `#email`.
  - `base64_round_trip`: `base64_decode(&base64_encode(b"hello\x89"))` == `b"hello\x89"`.
  - Extend `agent_script_parses` with `assert!(AGENT_JS.contains("locatorOf"))`.
- [ ] **Step 2: Run** `cargo test -p sawit-core browser::` — Expected: FAIL to compile (`Locator` undefined).
- [ ] **Step 3: Implement the Rust side**, moving `base64_decode` and fixing `sawit-mcp` callers (`target()` in `crates/mcp/src/browser.rs` and its tests use `..Default::default()`).
- [ ] **Step 4: Implement the page agent side** in `browser_agent.js`, next to `resolve`:
  - `deepAll()` — every element in document order, descending into open shadow roots (as `deepQuery` does).
  - `testidOf(el)` — first of the four attributes present.
  - `matchesRoleName(el, r, name)` — `shown(el) && role(el) === r && nameOf(el, r) === name` (the same `role`/`nameOf` the snapshot uses, unclipped).
  - `locatorOf(el)` — `{testid, role, name (clip 200), nth, css}`; `nth` = index of `el` in `deepAll().filter(matchesRoleName)`; `css` = `"#" + CSS.escape(el.id)` when `document.querySelectorAll` of it has length 1 and the id fails `/\d{3,}|^:r|^[0-9a-f-]{16,}$/i`.
  - `resolve(op)`: after `ref` and before `selector`, handle `op.locator`: a test id matching exactly one shown element wins; else the `nth` match of role + name; else `css`; else throw `"Nothing on the page is " + describe-like text + "."` (mirror `Locator::describe`).
  - `query(op)`: with `op.locator`, `found` also requires that resolution succeed and the element be shown.
- [ ] **Step 5: Run** `cargo test -p sawit-core browser::` and `cargo test -p sawit-mcp` — Expected: PASS.
- [ ] **Step 6: Commit** `feat(browser-test): stable element locators in the page agent`.

---

### Task 2: Test files

**Files:**
- Create: `crates/core/src/browser_test/mod.rs`, `crates/core/src/browser_test/spec.rs`
- Modify: `crates/core/src/lib.rs` (`pub mod browser_test;`)
- Test: `crates/core/src/browser_test/spec.rs` (`mod tests`)

**Interfaces:**
- Produces:

```rust
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TestSpec {
    pub name: String,          // file stem
    pub title: String,
    pub start: String,         // default "/"
    pub allow_console_errors: bool,
    pub steps: Vec<Step>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Step {
    pub line: usize,           // 1-based line of the numbered item
    pub text: String,
    pub notes: Vec<String>,
    pub expect: Vec<Expect>,
    /// `Open <url>` steps: the url or path.
    pub open: Option<String>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Expect {
    Text(String), NoText(String),
    Element { role: String, name: String },
    UrlContains(String), Url(String), Title(String),
    NoConsoleErrors, NoFailedRequests,
}
#[derive(Debug, Clone, PartialEq)]
pub struct SpecError { pub line: usize, pub message: String } // Display: "line {line}: {message}"

pub fn parse(name: &str, source: &str) -> Result<TestSpec, SpecError>;
impl Expect { pub fn describe(&self) -> String; } // the line after "expect ": `text "1 item"`, `button "Pay"`, `no console errors`
impl Step {
    /// 16 lowercase hex chars: FNV-1a 64 of text, notes and `describe()` of each expectation, joined by '\n'.
    pub fn key(&self) -> String;
}
/// `{{NAME}}` → value. Err(name) for the first missing one.
pub fn fill(text: &str, vars: &BTreeMap<String, String>) -> Result<String, String>;
impl TestSpec {
    /// The spec with every step text, note, expectation and `start` filled.
    /// Err: `Set {NAME} in .sawit/tests/.env or the environment.`
    pub fn filled(&self, vars: &BTreeMap<String, String>) -> Result<TestSpec, String>;
}
```

- [ ] **Step 1: Write the failing tests:**
  - `parses_the_spec_example`: the Markdown example from the spec parses to title `Checkout with a coupon`, start `/products`, `allow_console_errors: true`, 4 steps; step 2 has notes `["The coupon field is under \"Have a code?\""]` and `expect == [Text("10% off")]`; step 3 has `[UrlContains("/thanks"), Element { role: "heading", name: "Thank you" }, NoFailedRequests]`; step 4 has `open == Some("/account/orders")`.
  - `defaults_and_forms`: no headers → start `/`, `allow_console_errors: false`; `1)` numbering works; `- expect: text "x"` (colon) equals `- expect text "x"`; `expect no text "x"`, `expect url "/a"`, `expect title "Shop"`, `expect no console errors` each map to their variant.
  - `errors_name_the_line`: missing title → line 1, message contains `title`; no steps → message contains `step`; `Owner: me` header → contains `Owner`; `expect looks nice` → message lists the forms (contains `expect text`); an unterminated quote → contains `quote`; each with the right `line`.
  - `keys_follow_the_step`: same text → same key; changing a note or an expectation changes the key; changing another step doesn't; key is 16 hex chars.
  - `variables`: `fill("Pay {{CARD}}", {CARD: "4242"})` == `Ok("Pay 4242")`; missing → `Err("CARD")`; `filled` fills `start`, notes and `Expect::Text`; missing in `filled` → `Err("Set CARD in .sawit/tests/.env or the environment.")`.
- [ ] **Step 2: Run** `cargo test -p sawit-core browser_test::spec` — Expected: FAIL to compile.
- [ ] **Step 3: Implement** a line-based parser (no Markdown crate): title from the first `# ` line; `Key: value` lines until the first step; a step starts at `^\s*\d+[.)]\s+`; indented `-`/`*` bullets under it are expectations (after `expect`, optional `:`) or notes; other indented non-bullet lines continue the step text with a space. Quoted strings use `"` with `\"` escapes. A role is `[a-z]+` not one of `text`, `no`, `url`, `title`.
- [ ] **Step 4: Run** — Expected: PASS.
- [ ] **Step 5: Commit** `feat(browser-test): test file format`.

---

### Task 3: Actions and recordings

**Files:**
- Create: `crates/core/src/browser_test/record.rs`
- Test: same file, `mod tests`

**Interfaces:**
- Consumes: `Locator`, `BrowserOp`, `normalize_address` (Task 1); `TestSpec`, `Step::key`, `fill` (Task 2).
- Produces:

```rust
pub const RECORDING_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "do", rename_all = "snake_case")]
pub enum Action {
    Navigate { url: String }, Back, Forward, Reload,
    Click { locator: Locator, #[serde(default)] double: bool, #[serde(default, skip_serializing_if = "Option::is_none")] button: Option<String> },
    Hover { locator: Locator },
    Type { #[serde(default, skip_serializing_if = "Option::is_none")] locator: Option<Locator>, text: String, #[serde(default)] clear: bool, #[serde(default)] submit: bool },
    Press { key: String },
    Select { locator: Locator, values: Vec<String> },
    Scroll { #[serde(default, skip_serializing_if = "Option::is_none")] locator: Option<Locator>, #[serde(default)] dx: f64, #[serde(default)] dy: f64 },
    WaitFor { #[serde(default, skip_serializing_if = "Option::is_none")] text: Option<String>, #[serde(default)] gone: bool, #[serde(default, skip_serializing_if = "Option::is_none")] selector: Option<String> },
    Sleep { ms: u64 },
}
impl Action {
    /// The op to send, with `{{vars}}` filled and paths joined to `base_url`; None for WaitFor and Sleep.
    pub fn op(&self, base_url: &str, vars: &BTreeMap<String, String>) -> Result<Option<BrowserOp>, String>;
    /// Click, Press, Type with submit, and the four navigations.
    pub fn may_navigate(&self) -> bool;
    /// For reports and errors: `click button “Pay”`, `type "{{EMAIL}}" into textbox “Email”`.
    pub fn describe(&self) -> String;
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RecordedStep { pub text: String, pub actions: Vec<Action>, pub recorded_ms: u64 }

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Recording { pub version: u32, pub steps: BTreeMap<String, RecordedStep> }
impl Default for Recording; // version 1, no steps
impl Recording {
    /// A missing file is an empty recording; another version is an error naming the file.
    pub fn load(path: &Path) -> Result<Recording, String>;
    pub fn save(&self, path: &Path) -> Result<(), String>;
    /// Drops steps whose key isn't in `spec`.
    pub fn prune(&mut self, spec: &TestSpec);
    /// How many of `spec`'s non-Open steps have a recording.
    pub fn recorded(&self, spec: &TestSpec) -> usize;
}

#[derive(Debug, Clone, PartialEq)]
pub struct Recorded { pub action: Action, /** A password stored as typed. */ pub secret: bool }
/// What an executed op and the page's answer become in a recording; None for ops that aren't recorded
/// (snapshot, screenshot, console, network, evaluate, state, query, settled, devtools) or answers without a locator.
pub fn action_from(op: &BrowserOp, answer: &Value, base_url: &str, vars: &BTreeMap<String, String>) -> Option<Recorded>;
/// `url` relative to `base_url` when under it (`/a?b`), else as is.
pub fn relative_to(base_url: &str, url: &str) -> String;
/// A path joined to `base_url`; absolute URLs as they are.
pub fn join_url(base_url: &str, url: &str) -> String;
```

- [ ] **Step 1: Write the failing tests:**
  - `actions_round_trip`: a `Recording` with one step of each `Action` variant saves and loads equal; the file ends with `\n`, contains `"version": 1` and `"do": "click"`.
  - `load_missing_and_other_versions`: missing path → `Recording::default()`; a file with `"version": 2` → `Err` containing the file name.
  - `prune_keeps_current_steps`: recording keyed by the keys of a 3-step spec plus one stale key; after editing step 2's text and `prune`, steps 1 and 3 remain, 2 and the stale key are gone; `recorded()` == 2.
  - `ops_fill_and_join`: `Navigate { url: "/cart" }.op("http://localhost:5173", &vars)` == `Navigate { url: "http://localhost:5173/cart" }`; `Type { text: "{{EMAIL}}", .. }` fills from vars; missing var → `Err` with the spec's sentence; `WaitFor`/`Sleep` → `Ok(None)`.
  - `recording_from_tool_calls`: click answer `{"locator":{"role":"button","name":"Add","nth":1}}` → `Click` with that locator, `secret: false`; navigate to `localhost:5173/cart?x=1` with base `http://localhost:5173` → `Navigate { url: "/cart?x=1" }`; type `"a@b.co"` with vars `{EMAIL: "a@b.co"}` → text `{{EMAIL}}`; type with answer `"secret": true` and text matching no var → `secret: true`, text kept; `Snapshot` and `Evaluate` → `None`; click answer without `locator` → `None`.
  - `url_helpers`: `relative_to("http://localhost:5173", "http://localhost:5173/")` == `/`; another host stays absolute; `join_url` with an absolute URL returns it; base with trailing `/` doesn't double the slash.
- [ ] **Step 2: Run** `cargo test -p sawit-core browser_test::record` — Expected: FAIL to compile.
- [ ] **Step 3: Implement.** Sort is given by `BTreeMap`; `save` writes `serde_json::to_string_pretty` + `\n` via a temp file and rename.
- [ ] **Step 4: Run** — Expected: PASS.
- [ ] **Step 5: Commit** `feat(browser-test): recorded actions`.

---

### Task 4: The `Driver` trait and shared polling

**Files:**
- Create: `crates/core/src/browser_test/driver.rs`, `crates/core/src/browser_test/fake.rs`
- Modify: `crates/core/src/app_control.rs` (`pub const REPLY_TIMEOUT: Duration = Duration::from_secs(40);`), `crates/mcp/src/app.rs` (use it), `crates/mcp/src/browser.rs` (drop its own `POLL`/`SETTLE`/timeouts, `wait_loaded`, `after_action` polling and the `browser_wait_for` loop in favour of `driver`'s)
- Test: `crates/core/src/browser_test/driver.rs`; existing `crates/mcp` tests

**Interfaces:**
- Produces:

```rust
pub const POLL: Duration = Duration::from_millis(200);
pub const SETTLE: Duration = Duration::from_millis(250);
pub const DEFAULT_LOAD_TIMEOUT: Duration = Duration::from_secs(15);
pub const MAX_TIMEOUT: Duration = Duration::from_secs(30);

pub trait Driver: Send + Sync {
    fn op(&self, op: BrowserOp) -> impl Future<Output = Result<Value, String>> + Send;
}
/// Ops through the running app's control socket, for the window holding `project`.
#[derive(Debug, Clone)]
pub struct AppDriver { pub paths: AppPaths, pub project: Option<PathBuf> }
impl Driver for AppDriver; // app_control::send in spawn_blocking with REPLY_TIMEOUT

/// The final `State`; `"timed_out": true` added when still loading at `limit`.
pub async fn wait_loaded<D: Driver>(d: &D, limit: Duration) -> Result<Value, String>;
/// SETTLE, then wait_loaded(DEFAULT_LOAD_TIMEOUT); the state, or None when it failed.
pub async fn after_action<D: Driver>(d: &D) -> Option<Value>;
/// Polls `Query` until found (or gone); Ok(elapsed). Err is today's message:
/// "Timed out after {ms} ms waiting for “…”. Check browser_snapshot or browser_console."
pub async fn wait_for<D: Driver>(d: &D, text: Option<&str>, gone: bool, selector: Option<&str>, locator: Option<&Locator>, limit: Duration) -> Result<Duration, String>;
```

  `fake.rs` (`#[cfg(test)] pub(crate)`): `FakeDriver::new(f: impl Fn(&BrowserOp) -> Result<Value, String> + Send + Sync + 'static)`, `fn ops(&self) -> Vec<BrowserOp>` (every op received, in order). Polling uses `tokio::time::{sleep, Instant}` so tests run under `#[tokio::test(start_paused = true)]`.

- [ ] **Step 1: Write the failing tests** (`start_paused = true`):
  - `waits_for_the_load`: fake answers `State` with `loading: true` twice then `false`; `wait_loaded(.., 15 s)` returns the third state, after 3 `State` ops.
  - `load_times_out`: always loading → result has `"timed_out": true`.
  - `waits_for_text_to_go`: `Query` answers `found: true` then `false`; `wait_for(text "Saving", gone: true)` is `Ok`.
  - `wait_for_times_out_with_todays_message`: never found, limit 1 s → `Err` starting `Timed out after` and containing `“Saved”`.
- [ ] **Step 2: Run** `cargo test -p sawit-core browser_test::driver` — Expected: FAIL to compile.
- [ ] **Step 3: Implement** the driver module, then switch `crates/mcp/src/browser.rs` to it: an `impl Driver` for a small `ProjectDriver<'a> { sawit: &'a Sawit, project: &'a Option<String> }` that calls `app_call`, and the tools call `driver::wait_loaded` / `after_action` / `wait_for`. Tool behaviour and messages don't change.
- [ ] **Step 4: Run** `cargo test -p sawit-core browser_test && cargo test -p sawit-mcp` — Expected: PASS.
- [ ] **Step 5: Commit** `refactor(browser): load and wait polling shared through a Driver trait`.

---

### Task 5: Reports, files and the run lock

**Files:**
- Create: `crates/core/src/browser_test/report.rs`, `crates/core/src/browser_test/files.rs`, `crates/core/src/browser_test/lock.rs`
- Test: each file's `mod tests` (use `tempfile`)

**Interfaces:**
- Consumes: `ConsoleEntry`, `NetworkEntry`, `pick_dev_server`, `normalize_address` (browser); `TestSpec`, `parse`, `SpecError` (Task 2); `Recording::{load, recorded}` (Task 3).
- Produces:

```rust
// report.rs  (all Serialize + Deserialize, snake_case enums)
pub enum RunOutcome { Running, Passed, Failed, NeedsAgent, Aborted }
pub enum StepOutcome { Pending, Passed, Recorded, Healed, Failed, NeedsAgent, Skipped }
pub struct CheckResult { pub expect: String, pub passed: bool, pub detail: Option<String> }
pub struct StepReport {
    pub index: usize, // 1-based
    pub text: String, pub outcome: StepOutcome, pub duration_ms: u64,
    pub error: Option<String>, pub checks: Vec<CheckResult>,
    pub console_errors: Vec<ConsoleEntry>, pub failed_requests: Vec<NetworkEntry>,
    pub screenshot: Option<String>, // file name in the run dir
    pub note: Option<String>,       // the agent's bug note
}
pub struct RunReport {
    pub test: String, pub title: String, pub base_url: String,
    pub started_ms: u64, pub duration_ms: u64, pub outcome: RunOutcome,
    pub steps: Vec<StepReport>, pub warnings: Vec<String>,
}
impl RunReport {
    pub fn write(&self, run_dir: &Path) -> Result<(), String>; // report.json
    pub fn read(run_dir: &Path) -> Option<RunReport>;
    pub fn summary_markdown(&self) -> String;
}
impl StepOutcome { pub fn label(self) -> &'static str; } // "passed", "recorded", "healed", "failed", "needs agent", "skipped", "pending"

// files.rs
pub const TESTS_DIR: &str = ".sawit/tests";
pub const KEEP_RUNS: usize = 10;
pub fn tests_dir(project: &Path) -> PathBuf;
pub fn test_path(project: &Path, name: &str) -> PathBuf;      // <name>.md
pub fn recording_path(project: &Path, name: &str) -> PathBuf; // <name>.steps.json
pub fn runs_dir(project: &Path, name: &str) -> PathBuf;
pub struct TestEntry { pub name: String, pub path: PathBuf, pub spec: Result<TestSpec, SpecError>, pub recorded: usize, pub last: Option<RunReport> }
pub fn list_tests(project: &Path) -> Vec<TestEntry>; // *.md in tests_dir, by name; missing dir → empty
pub fn latest_report(project: &Path, name: &str) -> Option<RunReport>;
/// `.sawit/tests/.env` over `env` (pass `std::env::vars()`).
pub fn load_vars(project: &Path, env: impl IntoIterator<Item = (String, String)>) -> BTreeMap<String, String>;
/// Creates runs_dir/<started_ms>, removes all but the newest KEEP_RUNS, writes the .gitignore when missing.
pub fn new_run_dir(project: &Path, name: &str, started_ms: u64) -> Result<PathBuf, String>;
/// `given` normalized (no trailing slash), else the project's single dev server, else
/// Err("Start the dev server or give a base URL.").
pub async fn base_url(given: Option<&str>, project: &Path) -> Result<String, String>;

// lock.rs
pub struct RunLock { /* path */ }
impl RunLock {
    /// Err("A browser test is already running (pid {pid}).") when a live process holds it;
    /// a lock whose pid is gone is taken over.
    pub fn acquire(project: &Path) -> Result<RunLock, String>;
}
impl Drop for RunLock; // removes the file
```

- [ ] **Step 1: Write the failing tests:**
  - `summary_lists_healed_first`: a report with steps passed, healed, failed (error `"No button “Pay”"`) → the summary has the title, `failed`, the base URL, a "Healed" line before the step list, and the error text in full.
  - `report_round_trips`: `write` then `read` equal; `read` of an empty dir is `None`.
  - `lists_tests`: a dir with `a.md` (valid, 2 steps, 1 recorded in `a.steps.json`), `b.md` (invalid), `notes.txt` → two entries in order `a`, `b`; `a.recorded == 1`; `b.spec` is `Err`.
  - `vars_file_wins`: `.env` with `A=file`, `# c`, `B = spaced` and env `[("A","env"),("C","env")]` → `{A: file, B: spaced, C: env}`.
  - `keeps_ten_runs`: creating 12 run dirs leaves the newest 10; `.sawit/tests/.gitignore` is `.runs/\n.env\n`; an existing `.gitignore` is left alone.
  - `base_url_given`: `base_url(Some("localhost:5173/"), dir)` == `Ok("http://localhost:5173")`.
  - `lock_is_exclusive_and_recovers`: second `acquire` while the first is held → `Err` containing `already running`; after drop, `acquire` succeeds; a lock file containing pid `999999` (not running) is taken over.
- [ ] **Step 2: Run** `cargo test -p sawit-core browser_test` — Expected: FAIL to compile.
- [ ] **Step 3: Implement.** Liveness: `libc::kill(pid, 0) == 0` or `errno == EPERM`. The lock file is created with `create_new`; on `AlreadyExists`, read the pid, and if dead, remove and retry once.
- [ ] **Step 4: Run** — Expected: PASS.
- [ ] **Step 5: Commit** `feat(browser-test): reports, test files and the run lock`.

---

### Task 6: The runner

**Files:**
- Create: `crates/core/src/browser_test/runner.rs`
- Test: same file, `mod tests`, with `FakeDriver` and `start_paused = true`

**Interfaces:**
- Consumes: everything from Tasks 2–5.
- Produces:

```rust
pub const STEP_TIMEOUT: Duration = Duration::from_secs(5);

pub struct RunOptions {
    pub base_url: String,
    pub vars: BTreeMap<String, String>,
    pub run_dir: PathBuf,
    pub recording_path: PathBuf,
    pub step_timeout: Duration,
    /// Gets the report after every step (the app's live view).
    pub progress: Option<futures::channel::mpsc::UnboundedSender<RunReport>>,
}
#[derive(Debug, Clone, PartialEq)]
pub enum NeedReason {
    NotRecorded,
    ActionFailed { action: String, error: String },
    CheckFailed { check: String, detail: String },
}
impl Display for NeedReason; // "it has no recording yet" / "replaying “click button “Pay””: …" / "the check “text "1 item"” failed: …"
#[derive(Debug, Clone, PartialEq)]
pub enum Progress { Finished, NeedsAgent { step: usize /* 0-based */, reason: NeedReason } }
#[derive(Debug, Clone, PartialEq)]
pub enum AgentOutcome { Done, Bug(String), Abort }

pub struct Runner { /* raw spec, filled spec, recording, options, report, next step, open step */ }
impl Runner {
    /// Fills variables (Err: the spec's missing-variable sentence); report Running, steps Pending.
    pub fn new(spec: TestSpec, recording: Recording, opts: RunOptions) -> Result<Runner, String>;
    /// Opens Start, waits for the load, clears the console and network logs.
    pub async fn start<D: Driver>(&mut self, d: &D) -> Result<(), String>;
    /// Runs steps from the next one until the end (Finished, outcome Passed) or a step that needs the agent.
    pub async fn advance<D: Driver>(&mut self, d: &D) -> Progress;
    /// The open step: Done checks it (Err names the failing check; the step stays open),
    /// saves `actions` as its recording and continues like `advance`; Bug fails the step and the run
    /// with the note; Abort marks the run Aborted. Remaining steps become Skipped when the run ends early.
    pub async fn agent_finished<D: Driver>(&mut self, d: &D, outcome: AgentOutcome, actions: Vec<Recorded>) -> Result<Progress, String>;
    /// Ends a run left at a step that needs the agent (the app, with no agent): outcome NeedsAgent, rest Skipped.
    pub fn stop_needing_agent(&mut self);
    pub fn report(&self) -> &RunReport;
    pub fn spec(&self) -> &TestSpec; // filled
}
/// Lock, parse, recording, vars, run dir, base URL, Runner::new.
pub async fn prepare(project: &Path, name: &str, base_url: Option<&str>, progress: Option<UnboundedSender<RunReport>>) -> Result<(Runner, RunLock), String>;
/// prepare + start + advance; a step that needs the agent ends the run (stop_needing_agent).
pub async fn run_unattended<D: Driver>(d: &D, project: &Path, name: &str, base_url: Option<&str>, progress: Option<UnboundedSender<RunReport>>) -> Result<RunReport, String>;
```

  Per step (`advance`): Open → `Navigate` + `wait_loaded`. Recorded → each action: `op()`; retry the op on `Err` every `POLL` until `step_timeout`; `WaitFor` → `driver::wait_for(.., step_timeout)`; `Sleep` → sleep; after `may_navigate()` actions → `after_action`. Then each expectation polled until `step_timeout` (`Text`/`NoText`/`Element` via `Query`, `Url*`/`Title` via `State`); then `Console { level: "error", clear: true }` and `Network { failed_only: true, clear: true }` into the step report (console errors fail the step unless `allow_console_errors`; failed requests fail it only with `NoFailedRequests`); then `Screenshot` → `step-NN.png`; write `report.json` and send progress. A healed step whose `Recorded.secret` is set adds the warning `A password was recorded in {name}.steps.json; put it in .sawit/tests/.env as a variable.`

- [ ] **Step 1: Write the failing tests** (a fake page: a `State` with url/title, a set of texts `Query` finds, a click handler that changes them, a screenshot of `base64_encode(b"png")`):
  - `replays_a_recorded_test`: 2 recorded steps whose checks pass → `Progress::Finished`, outcome `Passed`, both steps `Passed`, `step-01.png` and `step-02.png` exist, `report.json` reads back equal to `report()`.
  - `new_step_needs_the_agent`: step 1 unrecorded → `NeedsAgent { step: 0, reason: NotRecorded }`; `agent_finished(Done, [click Add])` with the check now passing → step `Recorded`, the recording file now has step 1's key with that click, run `Passed`.
  - `stale_step_heals`: the recorded click's locator fails for 5 s → `NeedsAgent { reason: ActionFailed { .. } }` whose `action` is `click button “Add”`; after `agent_finished(Done, [click with nth 1])` → `Healed`, recording replaced.
  - `done_with_a_failing_check_keeps_the_step_open`: `agent_finished(Done, ..)` while `text "1 item"` is never found → `Err` containing `text "1 item"`; a second `agent_finished(Done, ..)` after the page changes succeeds and the recording holds the actions of the second call.
  - `bug_fails_the_run`: `agent_finished(Bug("Pay does nothing"))` → step `Failed` with `note`, later steps `Skipped`, outcome `Failed`.
  - `console_errors_fail_unless_allowed`: a step whose `Console` answer has one error → step `Failed` with it in `console_errors`; same with `allow_console_errors` → `Passed`. A failed request without `NoFailedRequests` → `Passed` with it listed.
  - `missing_variable_stops_before_start`: `Runner::new` with `{{CARD}}` unset → `Err("Set CARD in .sawit/tests/.env or the environment.")` and no op sent.
  - `secret_warning`: healing with `Recorded { secret: true, .. }` → `warnings` has the sentence with `shop.steps.json`.
  - `unattended_stops_at_needs_agent`: `run_unattended` on a test with an unrecorded step 2 of 3 → outcome `NeedsAgent`, step 2 `NeedsAgent`, step 3 `Skipped`, lock released (a new `RunLock::acquire` succeeds).
- [ ] **Step 2: Run** `cargo test -p sawit-core browser_test::runner` — Expected: FAIL to compile.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `cargo test -p sawit-core browser_test` — Expected: PASS.
- [ ] **Step 5: Commit** `feat(browser-test): replay runner`.

---

### Task 7: Agent tools

**Files:**
- Create: `crates/mcp/src/browser_test.rs`
- Modify: `crates/mcp/src/lib.rs` (`mod browser_test;`, `tests` field, `+ Self::browser_test_router()`, instructions line, expected tool names), `crates/mcp/src/browser.rs` (record after each successful action tool)
- Test: `crates/mcp/src/browser_test.rs` (`mod tests`), `crates/mcp/src/lib.rs` tool-name test, `crates/mcp/tests/tools.rs`

**Interfaces:**
- Consumes: `browser_test::{runner::{prepare, Runner, AgentOutcome, Progress, NeedReason}, record::{action_from, Action, Recorded}, files::{list_tests, base_url}, lock::RunLock, driver::Driver}`.
- Produces:

```rust
// lib.rs, on Sawit:
tests: Arc<tokio::sync::Mutex<Option<PausedRun>>>,

// browser_test.rs
pub(crate) struct PausedRun { project: PathBuf, project_arg: Option<String>, runner: Runner, _lock: RunLock, actions: Vec<Recorded> }
impl Sawit {
    /// Called by every browser_* action tool after the op succeeded: appended when a step is open.
    pub(crate) async fn record(&self, op: &BrowserOp, answer: &Value);
    /// browser_wait_for's waits.
    pub(crate) async fn record_action(&self, action: Action);
}
#[derive(Deserialize, JsonSchema)] pub struct TestListArgs { pub project: Option<String> }
#[derive(Deserialize, JsonSchema)] pub struct TestRunArgs { pub name: String, pub base_url: Option<String>, pub project: Option<String> }
#[derive(Deserialize, JsonSchema)] pub struct StepDoneArgs { /** done, bug or abort */ pub outcome: String, pub note: Option<String>, pub project: Option<String> }
/// The reply for a step that needs the agent (pure, tested).
fn needs_agent_message(spec: &TestSpec, step: usize, reason: &NeedReason, snapshot: &str) -> String;
```

  The project folder is `project` (with `~` expanded), else the MCP process's current directory. Tool descriptions:
  - `browser_test_list`: "List the project's browser tests (.sawit/tests/*.md): title, steps, how many are recorded, parse errors with line numbers, and the last run's outcome"
  - `browser_test_run`: "Run a browser test in Sawit's browser tab. Recorded steps replay without you; when a step is new or its recording no longer works, the answer gives you that step, its checks and the page: do it with the browser_* tools (they are recorded), then call browser_test_step_done"
  - `browser_test_step_done`: "Finish the browser test step that is waiting for you: outcome done (Sawit checks it, saves what you did, and carries on), bug (the app can't do it; say what is wrong in note), or abort"

  Instructions line (after the `browser_*` line): `- browser_test_*: plain-language tests in .sawit/tests/*.md; browser_test_run replays recorded steps and hands you the ones that are new or stale; finish each with browser_test_step_done.`

  `needs_agent_message` text: `Step {n} of {total} needs you ({reason}): “{text}”`, then notes as `- ` lines, then `Checks: ` with each `describe()`, then the instruction sentence from the `browser_test_run` description's second half, then `browser_evaluate isn't recorded: a step that only works through it can't be replayed.`, then a blank line and the snapshot (the same text `browser_snapshot` returns).

- [ ] **Step 1: Write the failing tests:**
  - In `lib.rs`: add `"browser_test_list"`, `"browser_test_run"`, `"browser_test_step_done"` to the expected names; `server_info_has_instructions` asserts `browser_test_run` appears.
  - `needs_agent_message_names_everything`: for the spec example's step 3 with `CheckFailed` → contains `Step 3 of 4 needs you`, `Pay with the test card`, `url contains "/thanks"`, `browser_test_step_done`, and the snapshot text.
  - In `tests/tools.rs`, `browser_test_list_reads_the_project`: a temp project with `.sawit/tests/a.md` (valid) and `b.md` (bad expect on line 4) → output contains `a`, `line 4`; and `browser_test_step_done` with no paused run → error text `No browser test step is waiting.`
  - `waiting_message_text`: `waiting_message("checkout", 2)` (0-based step) == `Step 3 of checkout is waiting for you: finish it with browser_test_step_done (outcome done, bug or abort).` `browser_test_run` returns this text when a step is open.
- [ ] **Step 2: Run** `cargo test -p sawit-mcp` — Expected: FAIL.
- [ ] **Step 3: Implement.** `browser_test_run`: refuse when paused (`waiting_message`); `prepare` → `start` → `advance` with a `ProjectDriver`; on `Finished` reply `report().summary_markdown()`; on `NeedsAgent` take a `Snapshot`, store the `PausedRun`, reply `needs_agent_message`. `browser_test_step_done`: take the paused run (none → `No browser test step is waiting.`), `agent_finished` with its `actions`; `Err` → put the run back and reply the error plus `Keep going, then call browser_test_step_done again.`; `NeedsAgent` again → store a fresh `PausedRun` with empty actions and reply as above; `Finished` → summary. Recording: in `browser.rs`, after each successful `navigate` (including back, forward and reload), `click`, `hover`, `type`, `press_key`, `select_option`, `scroll`, call `self.record(&op, &v)`; `browser_wait_for` calls `record_action(WaitFor | Sleep)` after it succeeds.
- [ ] **Step 4: Run** `cargo test -p sawit-mcp && cargo clippy --workspace --all-targets -- -D warnings` — Expected: PASS, no warnings.
- [ ] **Step 5: Commit** `feat(browser-test): browser_test_list, browser_test_run and browser_test_step_done`.

---

### Task 8: End-to-end check against a fixture

**Files:**
- Create: `docs/fixtures/browser/shop.html`, `docs/fixtures/browser/.sawit/tests/shop.md`

**Interfaces:**
- Consumes: the tools from Task 7, through Claude Code with Sawit's MCP server built from this branch.

`shop.html`: two products each with an "Add" button (the second cheaper), a cart line `role=status` showing `N item(s)`, a coupon field behind a "Have a code?" button, an `<input type=password id=card>` and a "Pay" button that navigates to `#thanks` and shows `<h1>Thank you</h1>`. `shop.md`:

```markdown
# Buy the cheaper product

Start: /shop.html

1. Add the cheaper product to the cart
   - expect text "1 item"
2. Apply the coupon {{COUPON}}
   - expect text "10% off"
3. Pay with card {{CARD}}
   - expect heading "Thank you"
```

- [ ] **Step 1:** Serve the fixture: `cd docs/fixtures/browser && python3 -m http.server 8765`; write `docs/fixtures/browser/.sawit/tests/.env` with `COUPON=SAVE10` and `CARD=4242424242424242`; start Sawit from this branch (`cargo run -p sawit`) with `docs/fixtures/browser` open.
- [ ] **Step 2:** From an agent: `browser_test_run name=shop base_url=http://localhost:8765 project=<abs path>` — Expected: step 1 needs you (no recording). Complete the 3 steps with `browser_*` + `browser_test_step_done outcome=done` — Expected: run `passed`, each step `recorded`; `shop.steps.json` has 3 steps, step 1 clicks `button “Add”` with `"nth": 1`, the card is stored as `{{CARD}}`; `.sawit/tests/.gitignore` exists.
- [ ] **Step 3:** Run again — Expected: `passed` with every step `passed` and no step handed to the agent.
- [ ] **Step 4:** Rename the second "Add" to "Add to cart" in `shop.html`, run — Expected: step 1 needs you (`ActionFailed`); fix it; run ends `passed` with step 1 `healed`, listed first in the summary; `git diff` shows the new locator.
- [ ] **Step 5:** Break the Pay button (remove its handler), run, call `browser_test_step_done outcome=bug note="Pay does nothing"` — Expected: `failed`, step 3 `failed` with the note.
- [ ] **Step 6:** Kill the agent's MCP process while a step waits; run again from a new agent — Expected: the run starts (dead pid lock taken over).
- [ ] **Step 7:** Restore `shop.html` to its original text; remove `.runs/`, `.env` and `shop.steps.json` (so the check starts unrecorded next time); commit `shop.html`, `shop.md` and `.sawit/tests/.gitignore`: `feat(browser-test): fixture for the end-to-end check`.
