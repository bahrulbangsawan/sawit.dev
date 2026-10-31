# Browser tests: design

End-to-end tests for the web app being built, written in plain language,
run in Sawit's browser tab, and kept fast and repeatable by recording what
an agent did. In the spirit of TesterArmy's e2e (https://e2e.tester.army/):
agent steps that are cached once they pass and replayed with no model call
until the app changes. Sawit itself never calls a model. The agent is the
one already driving Sawit over MCP (Claude Code, Codex, Cursor…).

## Why this approach

| Option | Who runs the model | Browser | Cost |
|---|---|---|---|
| **Record agent runs, replay in Rust (chosen)** | The user's own agent, only when a step is new or stale | The browser tab (WKWebView) that `browser_*` already drives | Builds on the browser tab and `sawit-mcp` that already exist; no API keys in Sawit |
| Port e2e's TypeScript framework to Rust | Sawit, with its own tool loop | Needs Playwright or CDP | A second agent loop, model settings and billing; most of e2e's value is Playwright |
| Add `e2e mcp` to the MCP catalog | e2e, through an AI SDK provider | Playwright's | A second set of browser tools beside `browser_*`; nothing in Sawit gets better |

Sawit already gives agents a real browser: 13 `browser_*` tools, an
accessibility snapshot with element refs, console and network capture
(see `docs/superpowers/specs/2026-10-29-browser-pane-design.md`). What is
missing is a test file, a way to record and replay steps, reports, and a
place in the app to run them. A headless runner for CI (Chrome over CDP)
is a later project; recordings are written so it can replay them.

## User-facing behaviour

### Test files

Tests live in the project at `.sawit/tests/<name>.md`. The file name
without `.md` is the test's name (`checkout`). The file is Markdown so it
reads well on GitHub and in Sawit's preview:

```markdown
# Checkout with a coupon

Start: /products
Allow: console errors

1. Add the cheapest product to the cart
   - expect text "1 item"
2. Apply the coupon {{COUPON}}
   - The coupon field is under "Have a code?"
   - expect text "10% off"
3. Pay with the test card 4242 4242 4242 4242
   - expect url contains "/thanks"
   - expect heading "Thank you"
   - expect no failed requests
4. Open /account/orders
   - expect text "Order #"
```

- **Title:** the first `# ` heading. Required.
- **Headers:** `Key: value` lines between the title and the first step.
  `Start:` is the page the test begins on: a path (joined with the base
  URL) or an absolute URL; default `/`. `Allow: console errors` stops
  console errors from failing steps. Any other key is an error.
- **Steps:** each numbered item (`1.`, `2)`, any number) is a step. Its
  text is what the agent is asked to do.
- **Notes:** bullets under a step that don't start with `expect` are hints
  for the agent. They are part of the step, so editing them counts as
  editing the step.
- **Open steps:** a step whose whole text is `Open <url-or-path>` is run by
  Sawit directly and never needs the agent.
- **Expectations:** bullets under a step starting with `expect` (a colon
  after it is allowed). Sawit checks them itself, so the grammar is fixed:

| Line | Passes when |
|---|---|
| `expect text "…"` | The page's visible text contains it (any case) |
| `expect no text "…"` | It doesn't |
| `expect <role> "<name>"` | A visible element has this role and accessible name, as `browser_snapshot` shows them (`button "Pay"`, `heading "Thank you"`, `link "Docs"`) |
| `expect url contains "…"` | The address contains it |
| `expect url "…"` | The address is exactly it (a path is joined with the base URL first) |
| `expect title "…"` | The document title contains it (any case) |
| `expect no console errors` | No console error or page error during the step (the default unless `Allow: console errors`) |
| `expect no failed requests` | No request in the step got status ≥ 400 or no response |

  Expectations are polled for up to **5 s** each, so a step doesn't fail
  while the page is still updating. A line that matches none of these is
  a parse error that lists the forms above.
- **Variables:** `{{NAME}}` in a step, note or expectation is replaced at
  run time from `.sawit/tests/.env` (`NAME=value` lines; `#` comments),
  then from the environment. A missing variable stops the run before it
  starts, naming the variable and both places to set it. Variables are for
  secrets (passwords, card numbers) and values that differ per machine.

### Recordings

Next to each test, `.sawit/tests/<name>.steps.json` holds what was
recorded for each step. It is meant to be committed: replays on another
machine need no agent, and a changed recording shows up in review.

- Steps are keyed by a hash of the step's text, notes and expectations
  (before variables are filled in). Editing a step drops its recording
  and leaves the others alone. Recordings of steps that are no longer in
  the file are removed on the next save.
- A recorded action names its element by a **locator**, never by a
  snapshot ref (refs reset on every snapshot). A locator holds, when the
  element has them: its test id (`data-testid`, `data-test`, `data-cy`,
  `data-qa`), its role and accessible name, which match it is in page
  order (`nth`) among visible elements with that role and name, and a
  `#id` selector when the id is unique and doesn't look generated (no
  runs of 3+ digits, no React `:r…:` ids, no long hex). Replay tries them
  in that order: test id, role + name + nth, then `#id`.
- Text typed into a field is recorded as typed, except that a value equal
  to a variable's value is recorded as `{{NAME}}`. Text typed into a
  password field that matches no variable is still recorded (the replay
  needs it), and the run report warns: "A password was recorded in
  checkout.steps.json; put it in .sawit/tests/.env as a variable."
- A navigation to an address under the base URL is recorded as a path,
  so replays work on another port.
- Recorded: navigate, back, forward, reload, click, hover, type, press
  key, select option, scroll, wait for. Not recorded: snapshot,
  screenshot, console, network, evaluate. A step that needs
  `browser_evaluate` to happen can't be replayed; the agent is told so.

### Running a test

A run:

1. Fails at once when the file doesn't parse, a variable is missing, or
   another run is going (one at a time per project, across the app and
   every agent).
2. Picks the base URL: the one given to the run, else the project's
   running dev server (exactly one dev server whose folder is in the
   project, as `browser_navigate` picks it), else it fails with "Start the
   dev server or give a base URL."
3. Opens `Start:`, waits for the load, and clears the console and network
   logs.
4. For each step, in order:
   - An Open step navigates and waits for the load.
   - A recorded step replays its actions. An action whose element isn't
     there yet is retried for up to 5 s. A click, a key press or a submit
     waits for any navigation it starts, as the `browser_*` tools do.
   - Then the expectations are checked, then console errors (unless
     allowed) and failed requests are collected for the step, then a
     screenshot is taken.
   - The step **needs the agent** when it has no recording, an action
     still fails after 5 s, or an expectation fails after replay. Without
     an agent (a run started in the app), the run stops there with
     outcome *needs agent*.
5. Ends **passed** when every step passed, **failed** when the agent
   reported a bug, and **aborted** when the agent gave up.

Each step ends as one of: *passed* (replayed, checks passed), *recorded*
(no recording before; the agent did it and the checks passed), *healed*
(the recording failed; the agent did it another way and the checks
passed), *failed*, *needs agent*, or *skipped* (after the run stopped).

A healed step can hide a real change (a button renamed on purpose, or by
mistake), so reports list healed steps first and the new recording shows
up as a diff in `<name>.steps.json`.

### Agent tools (`sawit-mcp`)

All take `project` like the other `browser_*` tools.

| Tool | Does |
|---|---|
| `browser_test_list` | Every test in `.sawit/tests`: name, title, step count, how many steps are recorded, a parse error if any, and the last run's outcome and time |
| `browser_test_run` | `name`, optional `base_url`. Replays until the end or until a step needs the agent. Returns the summary, or the step that needs the agent: its number, text, notes, expectations, why (not recorded / which action failed and how / which check failed), and a page snapshot |
| `browser_test_step_done` | `outcome`: `done` (the agent did the step), `bug` (the app can't do it; `note` says what is wrong), or `abort`. With `done`, Sawit checks the step's expectations: if they pass, it saves what the agent did as the step's recording and carries on as `browser_test_run` does; if not, the step stays open and the answer names the failing check |

While a step is open, every recordable `browser_*` call the agent makes
is recorded (only calls that succeed). `browser_test_run` while a step is
open fails: "Step 3 of checkout is waiting for you: finish it with
browser_test_step_done (outcome done, bug or abort)."

An agent writes tests with its own file tools; `browser_test_list` shows
parse errors with line numbers. The server instructions gain one line
about `browser_test_*`.

### Run reports

Each run writes `.sawit/tests/.runs/<name>/<unix-ms>/`: `report.json`,
updated after every step, and `step-NN.png` screenshots (`NN` from 01).
The newest 10 runs per test are kept. On the first run Sawit writes
`.sawit/tests/.gitignore` (`.runs/` and `.env`) if it doesn't exist.

A Markdown summary (title, outcome, base URL, duration, then healed
steps, then one line per step with failures in full) is what
`browser_test_run` returns and what the app's "Copy summary" copies.

### In the app

- **Run panel → Tests section** below the scripts: one row per test with
  its last outcome (icon and color), title and "4/5 recorded". A row's
  play button runs it; the section's play button runs every test, one
  after another. Clicking a row opens its report tab. A test that doesn't
  parse shows its error under the title and has no play button. The list
  reloads when the panel's project changes, from the panel's refresh
  button, and after every run.
- **Report tab** (one per test, titled with the test's title): outcome,
  base URL, start time and duration, warnings; buttons **Run again**,
  **Fix with agent** (when the outcome is *needs agent* or *failed*),
  **Open test file**, and **Copy summary**. Then one card per step:
  number, text, outcome, duration, error, each check with ✓/✗ and detail,
  console errors and failed requests (collapsed when there are none), and
  the screenshot as a thumbnail that opens in Sawit's image viewer. During
  a run the tab updates after every step.
- **Fix with agent** opens a terminal tab in the project running
  `claude "<prompt>"` (or `codex "<prompt>"` when only Codex is installed)
  with the prompt: *Run the Sawit browser test "checkout" with
  browser_test_run. When a step needs you, do it with the browser_* tools
  and call browser_test_step_done. If the app is what's broken, call it
  with outcome bug and a note saying what is wrong.* When neither CLI is
  installed, or Sawit's MCP server isn't set up in it, the button opens
  the MCP page instead.
- A run from the app brings the browser tab forward (screenshots need it
  showing), as `browser_navigate` does.

## Architecture

```
.sawit/tests/checkout.md ──parse──► TestSpec ─┐
.sawit/tests/checkout.steps.json ──► Recording ┼─► Runner::advance(driver) ──► Progress
                                               │        │  (Finished | NeedsAgent)
                                               │        ▼
                                               │  Driver::op(BrowserOp) ── AppDriver ──► app_control socket
                                               │                                         ──► BrowserView::apply
sawit-mcp browser_test_run ─► Runner (paused in the MCP process while the agent works)
sawit-mcp browser_click etc. ─► recorded as Actions ─► browser_test_step_done ─► Runner::agent_finished
app Run panel ─► Runner on the Tokio runtime with AppDriver ─► report.json ─► report tab
```

- `sawit_core::browser`: `Locator`, `Target.locator`, `Query.locator`,
  `base64_decode` (moved from `sawit-mcp`).
- `browser_agent.js`: `locatorOf(el)` added to the answers of click,
  hover, type, select and scroll (plus `secret: true` for password
  fields); `resolve` and `query` accept a locator.
- `sawit_core::browser_test` (new): `spec` (parse, variables, step keys),
  `record` (actions, recording file, turning a tool call into an action),
  `driver` (the `Driver` trait, `AppDriver`, the load and wait polling
  moved out of `sawit-mcp`), `runner`, `report`, `lock`, `files` (paths,
  `.env`, listing tests, keeping 10 runs).
- `sawit-mcp`: `browser.rs` uses `driver`'s polling; `browser_test.rs`
  (new) has the three tools and the paused run.
- App: `run/tests.rs` (the Tests section), `test_report.rs` (the report
  tab).

Both the MCP server and the app reach the browser through the control
socket (`AppDriver`), so the runner has one code path. The app's runner
runs on the Tokio runtime, never on the UI thread.

## Not in this project

- CI and a headless browser: a `sawit test` command that replays
  recordings in Chrome over CDP and writes JUnit. Recordings already use
  locators, not WebKit-specific state, so it can replay them.
- Expectations judged by a model ("the page looks right"). Sawit never
  calls a model.
- An explore mode that looks for bugs without a test.
- Firefox, Chromium, mobile.
- Several runs at once.
