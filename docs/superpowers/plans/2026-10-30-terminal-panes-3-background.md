# Terminal panes 3: background sessions in the app — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Every terminal in the dock runs in the terminal service, so closing a window or quitting Sawit leaves processes running, and the next window picks the tabs up again with their panes and scrollback.

**Architecture:** `TerminalView` draws any `TermBackend`. An app-global `TerminalHost` owns the one `ServiceClient` (or falls back to in-process sessions when the service cannot start) and spawns panes. Each `TerminalTab` mirrors itself to the service as a `TabRecord` and claims it for its window; a panel releases its tabs when its window goes away and adopts detached tabs when it opens or learns its projects. A "Background" menu in the dock header lists tabs this window isn't showing.

**Tech Stack:** Rust 2024, GPUI via `gpui-kit` 0.8.1, plan 2's `sawit_core::terminal::service`.

**Spec:** `docs/superpowers/specs/2026-10-30-terminal-split-panes-design.md` (sections "Background sessions", "Existing callers").

**Series:** 3 of 3. Needs plans 1 and 2.

## Global Constraints

- Window close and app quit **detach**; only Close (tab), Close pane, End, and End All kill. No quit prompt.
- A tab is shown in one window at a time (its `TabRecord.owner`).
- On window open: adopt detached tabs whose `group` is one of the window's projects (in `created_unix` order) and detached tabs with no group; start a fresh shell only if nothing was adopted.
- Run panel scripts and SSH sessions persist like shells.
- If the service cannot start, terminals run in-process and the dock shows once: "Terminals will close when Sawit quits: {error}".
- Background menu copy: button tooltip "Background Terminals"; per tab "Show Here", "End"; footer "End All Background Terminals"; older service row "Terminals from an older Sawit are still running" with "End Them".
- Windows (non-Unix) stays in-process (`TerminalHost::local()`).
- Commits: `feat(terminal): …`, ending with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.

## Review Focus

1. The service cannot be started (bad exe path, unwritable folder): terminals still open in-process with the one-time banner — test in Task 1.
2. Two windows on the same project at launch: the first adopts the tabs, the second starts its own shell — test in Task 4.
3. A tab whose process exited while detached: it is adopted and shows the exited banner rather than vanishing — test in Task 4.
4. The service dies while windows are open: every pane shows exited, a banner explains it, and the next New Terminal starts a new service — test in Task 6.
5. The first project arrives after the panel already opened its fresh shell: adopted project tabs replace that untouched shell instead of sitting beside it — test in Task 4.

---

## File Structure

| File | Responsibility |
|---|---|
| `crates/app/src/terminal/host.rs` (new) | `TerminalHost` global: service client or in-process, spawning, notices |
| `crates/app/src/terminal/view.rs` | draw and drive `Rc<dyn TermBackend>` |
| `crates/app/src/terminal/tab.rs` | `TabRecord` sync, restore from a record, kill vs detach |
| `crates/app/src/terminal/panel.rs` | adopt/release, Background menu, service-lost banner |
| `crates/app/src/terminal/background.rs` (new) | Background menu labels and rows (pure parts tested) |
| `crates/app/src/terminal/mod.rs` | `init` installs the host |
| `README.md` | terminal section: panes and background terminals |

---

### Task 1: Views over `TermBackend`, spawned by `TerminalHost`

**Files:**
- Create: `crates/app/src/terminal/host.rs`
- Modify: `crates/app/src/terminal/view.rs`, `crates/app/src/terminal/panel.rs`, `crates/app/src/terminal/mod.rs`
- Test: `crates/app/src/terminal/host.rs`

**Interfaces:**
- Consumes: plan 2 `TermBackend`, `ServiceClient`, `RemoteSession`, `AppPaths::terminal_socket`, `PROTOCOL`; plan 1 `PaneId`.
- Produces:
  - `TerminalView::new(session: Rc<dyn TermBackend>, events: UnboundedReceiver<TermEvent>, title, cx)`; every `self.session.…` call goes through the trait; painting uses `session.snapshot(&palette)` as today. `TerminalView::kill(&self)` calls `session.kill()`.
  - ```rust
    pub struct TerminalHost { /* starter, RefCell<Option<ServiceClient>>, fallback error, notice subscribers */ }
    impl gpui_kit::Global for TerminalHost {}
    impl TerminalHost {
        /// Connects (starting the service if needed) on first use via `start`.
        pub fn new(start: impl Fn() -> io::Result<ServiceClient> + 'static) -> Self;
        /// Always in-process.
        pub fn local() -> Self;
        pub fn spawn(&self, pane: PaneId, cmd: Command, size: GridSize) -> Result<(Rc<dyn TermBackend>, UnboundedReceiver<TermEvent>), String>;
        pub fn attach(&self, pane: PaneId) -> Option<(Rc<dyn TermBackend>, UnboundedReceiver<TermEvent>)>;
        pub fn client(&self) -> Option<ServiceClient>;
        /// Set once when the service could not start; the panel shows it once.
        pub fn take_fallback_notice(&self) -> Option<String>;
    }
    ```
    `spawn` tries the client; if `start` fails it remembers the error, uses `Session::spawn`, and keeps trying the service again on later spawns no more than once a minute.
  - `terminal::init` sets the global: on macOS `TerminalHost::new(|| ServiceClient::connect_or_start(&AppPaths::default_macos().terminal_socket(PROTOCOL), &std::env::current_exe()?))`, else `TerminalHost::local()`.
  - `TerminalPanel::spawn` takes the pane id from `new_pane_id()` and calls `cx.global::<TerminalHost>().spawn(...)`.
- Test helper (in `host.rs` tests, reused by later tasks): `fn service_host() -> (TempDir, Stopper, TerminalHost)` — binds a `Service` on a temp socket in a thread (grace 300 ms) and returns `TerminalHost::new(move || ServiceClient::connect(&socket))`.

- [ ] **Step 1: Write the failing tests:**

```rust
#[gpui_kit::test]
fn a_panel_runs_its_shell_in_the_service(cx: &mut TestAppContext) {
    // set the service_host() global; open_panel; write "echo via-service\n" to the focused view;
    // poll its text_tail for "via-service"; then prove it runs in the service:
    // a second ServiceClient::connect(socket).attach(focused pane id) shows "via-service" too.
}

#[gpui_kit::test]
fn without_a_service_terminals_run_in_process_and_say_so(cx: &mut TestAppContext) {
    // global = TerminalHost::new(|| Err(io::Error::other("no service")));
    // open_panel → one live tab; host.take_fallback_notice() == Some("no service") is shown by the panel
    // as "Terminals will close when Sawit quits: no service" (assert the panel's notice string).
}
```

- [ ] **Step 2: Run** `cargo test -p sawit terminal::` — Expected: FAIL.
- [ ] **Step 3: Implement.** Existing view/panel tests keep using `TerminalHost::local()` unless they test the service.
- [ ] **Step 4: Run** `cargo test -p sawit` — Expected: PASS.
- [ ] **Step 5: Commit** `feat(terminal): terminals run in the terminal service`.

---

### Task 2: Tabs mirror themselves to the service

**Files:**
- Modify: `crates/app/src/terminal/tab.rs`, `crates/app/src/terminal/panel.rs`
- Test: `crates/app/src/terminal/tab.rs`

**Interfaces:**
- Consumes: plan 2 `TabRecord`, `PaneRecord`, `PaneKind`, `Owner`, `TabId`.
- Produces:
  - `TerminalTab` gains `pub id: TabId` (from `new_pane_id()`), `created_unix: u64`, and `window: u64` (the panel's window id; `TerminalPanel` draws one random `window_id: u64` in `new`).
  - `impl TerminalTab { pub fn record(&self, cx: &App) -> TabRecord }` — `owner: None` (the service fills it), `panes` from each view: `remote` → `PaneKind::Ssh { key, name, launch: ssh_command(&launch) }`; `job` → `Job { key, name }`; `shell` → `Shell`; else `Program`; `title` = the view's title.
  - `fn sync(&self, cx: &App)` — `put_tab(self.record(cx))` when the host has a client; called after every `TabState` change, pane add/close, title change and `group` change. A new tab also calls `claim_tab(id, window)`.
  - Closing: `TerminalPanel::close(ix)` calls `remove_tab(id)` (kills every pane) before dropping the tab; `TerminalTab::close_pane` calls `view.kill()` then `sync`. In-process (no client), behaviour is unchanged.

- [ ] **Step 1: Write the failing tests:**

```rust
#[gpui_kit::test]
fn a_split_tab_is_recorded_in_the_service(cx: &mut TestAppContext) {
    // service_host(); open_panel; cmd-d; rename the focused pane "api";
    // list_tabs() from a second client: one record, 2 panes, layout Split Right,
    // panes[focused].title irrelevant but state.panes[focused].name == Some("api"),
    // owner == Some(Owner { client: host client id, window: panel.window_id }).
}

#[gpui_kit::test]
fn closing_a_tab_ends_its_processes(cx: &mut TestAppContext) {
    // get the shell pid via "echo $$"; panel.close(0); poll until libc::kill(pid, 0) == -1; list_tabs() empty.
}
```

- [ ] **Step 2: Run** `cargo test -p sawit terminal::` — Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** — Expected: PASS.
- [ ] **Step 5: Commit** `feat(terminal): keep tab layouts in the terminal service`.

---

### Task 3: Closing a window detaches

**Files:**
- Modify: `crates/app/src/terminal/panel.rs`
- Test: `crates/app/src/terminal/panel.rs`

**Interfaces:**
- Produces: in `TerminalPanel::new`, `cx.on_release(|panel, cx| …)` calls `release_tab(id)` for each tab; dropping the views drops their `RemoteSession`s, which only detach. (App quit closes the socket; the service clears owners itself.)

- [ ] **Step 1: Write the failing test:**

```rust
#[gpui_kit::test]
fn closing_the_window_leaves_the_shell_running(cx: &mut TestAppContext) {
    // service_host(); open_panel; "echo $$" → pid; "echo still-here\n";
    // close the window (cx.update_window(w, |_, window, _| window.remove_window())); run_until_parked;
    // list_tabs(): one record, owner None; libc::kill(pid, 0) == 0;
    // a second client attach(pane) → snapshot text contains "still-here".
}
```

- [ ] **Step 2: Run** — Expected: FAIL (the session is killed or the owner stays set).
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `cargo test -p sawit terminal::` — Expected: PASS.
- [ ] **Step 5: Commit** `feat(terminal): closing a window keeps its terminals running`.

---

### Task 4: Opening a window adopts detached tabs

**Files:**
- Modify: `crates/app/src/terminal/panel.rs`, `crates/app/src/terminal/tab.rs`
- Test: `crates/app/src/terminal/panel.rs`

**Interfaces:**
- Produces:
  - `impl TerminalTab { pub fn restore(record: TabRecord, window: u64, cx: &mut Context<Self>) -> Option<Self> }` — `attach`es every pane in `record.state.layout.panes()`; builds each view with the record's title and its `remote`/`job`/`shell` from `PaneKind`; `None` if no pane could be attached (the panel then calls `remove_tab`). Claims the tab.
  - `fn adopt(&mut self, groups: Adopt, window: Option<&mut Window>, cx: &mut Context<Self>) -> usize` with `enum Adopt { Ungrouped, Projects(Vec<PathBuf>) }` — `list_tabs`, keep records with `owner == None` matching, sorted by `created_unix`, restore and insert each; returns how many.
  - `TerminalPanel::new`: `adopt(Adopt::Ungrouped)`; open a fresh shell only if it returned 0.
  - `set_projects`: on each call `adopt(Adopt::Projects(new projects not seen before))`. On the first project, if any project tab was adopted, kill and remove fresh shells that are untouched (`!touched`, at prompt, no job/remote) instead of moving them into the project; otherwise keep today's behaviour.
  - An adopted pane whose program already exited gets its `Exit` event on attach and shows the existing exited banner.

- [ ] **Step 1: Write the failing tests:**

```rust
#[gpui_kit::test] fn a_new_window_adopts_its_projects_tabs(cx)
    // window 1: set_projects([P]), split, echo "kept"; close window 1;
    // window 2: open_panel, set_projects([P]) → tab_count() == 1, 2 panes, text contains "kept", and no extra fresh shell tab.
#[gpui_kit::test] fn another_windows_project_tab_is_not_taken(cx)
    // window 1 open on P with a tab; window 2 opens on P → window 2 has only its own fresh shell; window 1 still has its tab.
#[gpui_kit::test] fn an_ungrouped_tab_goes_to_the_next_window(cx)
    // window 1 with no project; close; window 2 (no project) adopts it.
#[gpui_kit::test] fn a_tab_that_exited_while_detached_is_adopted_exited(cx)
    // window 1 on P: a tab running Command { program: "/bin/sh", args: ["-c", "echo done; sleep 0.5"], cwd: P };
    // close window 1; wait 1 s; window 2 on P adopts it; the view is exited and its text contains "done".
```

- [ ] **Step 2: Run** — Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `cargo test -p sawit terminal::` — Expected: PASS.
- [ ] **Step 5: Commit** `feat(terminal): new windows pick up running terminals`.

---

### Task 5: The Background menu

**Files:**
- Create: `crates/app/src/terminal/background.rs`
- Modify: `crates/app/src/terminal/panel.rs`
- Test: `crates/app/src/terminal/background.rs`, `crates/app/src/terminal/panel.rs`

**Interfaces:**
- Produces:
  - `pub fn background_label(record: &TabRecord, now_unix: u64) -> String` — `"{project} · {title} · {n} pane(s) · {age}"`: project = folder name of `group` (`crate::tab_groups::project_name`) or "No project"; title = the focused pane's `PaneMeta.name`, else its `PaneRecord.title`; "1 pane" / "N panes"; age "<1m", "{m}m", "{h}h", "{d}d".
  - `pub fn background_tabs(records: Vec<TabRecord>, window: u64, client: u64) -> Vec<TabRecord>` — records not owned by `(client, window)`, oldest first.
  - Dock header button (ghost xsmall, `Lucide::Layers`, label = count, tooltip "Background Terminals"), shown when the count > 0 or an older service is alive. The count refreshes every 2 s (a `cx.spawn` timer calling `list_tabs` off the main thread via the host's runtime) and on notices.
  - Menu (built on open from a fresh `list_tabs`): one submenu per tab titled `background_label`, with "Show Here" (`claim_tab` + `restore` + insert + activate) and "End" (`remove_tab`); separator; "End All Background Terminals" (`remove_tab` for each listed tab). When `older_services(terminal_dir)` is non-empty: a row "Terminals from an older Sawit are still running" with "End Them" (`end_service(pid)` each).
  - `TerminalHost` dispatches `ClientNotice::TabTaken { tab, by }` to every panel (subscriber list of `WeakEntity<TerminalPanel>`); a panel holding `tab` with `window != by.window || client != by.client` removes it **without** killing (drops its views; no `remove_tab`).

- [ ] **Step 1: Write the failing tests:**

```rust
#[test]
fn labels_read_like_the_spec() {
    // group /x/website, title "pricing page", 2 panes, created 12 min ago →
    // "website · pricing page · 2 panes · 12m"; no group, 1 pane, 30 s → "No project · zsh · 1 pane · <1m";
    // 3 h and 2 d cases.
}

#[test]
fn background_tabs_skip_this_windows_own() { /* owners: None, (c,w), (c,other), (other,w) → 3 kept */ }

#[gpui_kit::test]
fn show_here_moves_a_tab_between_windows(cx: &mut TestAppContext) {
    // windows 1 and 2 (same host); window 1 has a tab with output "moved";
    // window 2 "Show Here" on it → window 2 has it with "moved" on screen; window 1 no longer lists it; the process is alive.
}
```

- [ ] **Step 2: Run** — Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `cargo test -p sawit terminal::` — Expected: PASS. Manual: two Sawit windows, move a tab back and forth, End one, End All.
- [ ] **Step 5: Commit** `feat(terminal): background terminals menu`.

---

### Task 6: Scripts, SSH and a lost service

**Files:**
- Modify: `crates/app/src/terminal/panel.rs`, `crates/app/src/terminal/host.rs`
- Test: `crates/app/src/terminal/panel.rs`

**Interfaces:**
- Produces:
  - Adopted Job and SSH panes are found by `job_status`/`start_job`/`stop_job`/`show_job` and `ssh_sessions`/`focus_ssh` (they come back with their tags from Task 4's restore; this task proves it).
  - On `ClientNotice::Disconnected`: the host drops its client (next `spawn` calls `start` again); each panel shows `Banner::info("term-service-lost", "The terminal service stopped, so these terminals ended. New terminals start it again.")`, dismissible.

- [ ] **Step 1: Write the failing tests:**

```rust
#[gpui_kit::test] fn a_running_script_is_found_after_reopening(cx)
    // window 1 on P: start_job(tag "dev", P, "sleep 30"); close window; window 2 on P:
    // job_status("dev") == Some(JobStatus { running: Some(_) }); stop_job("dev") → running becomes None.
#[gpui_kit::test] fn losing_the_service_ends_panes_and_recovers(cx)
    // service_host whose start closure binds a fresh Service each call; stopper.stop();
    // every view exited, banner shown; open_shell() → a live tab again.
```

- [ ] **Step 2: Run** — Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `cargo test -p sawit` — Expected: PASS.
- [ ] **Step 5: Commit** `feat(terminal): scripts and ssh survive reopening; recover from a lost service`.

---

### Task 7: Docs and the end-to-end check

**Files:**
- Modify: `README.md` (terminal section: split panes and the pane menu, keys, and that terminals keep running after a window closes or Sawit quits, with the Background menu; design-notes row for the terminal service: one process per user, socket under the app-support folder, exits 10 s after its last terminal)

- [ ] **Step 1: Edit README.**
- [ ] **Step 2: Manual check, release build** (`cargo build --release -p sawit`, run the app):
  1. Split a tab, run `claude` in one pane and `htop` in the other; typing and redraws feel as fast as before.
  2. Quit Sawit (⌘Q). `ps -ax | grep -c "[c]laude"` still counts it.
  3. Open Sawit on the same project: the tab comes back split, both panes live, Claude Code mid-run.
  4. Open a second window, Background → Show Here moves the tab; End kills it.
  5. Close every terminal and quit; within ~10 s no `sawit` service process remains (`pgrep -fl sawit`).
- [ ] **Step 3: Run** `cargo test --workspace` and `cargo clippy --workspace -- -D warnings` — Expected: PASS.
- [ ] **Step 4: Commit** `feat(terminal): terminals keep running when Sawit quits` (the release notes come from this message).
