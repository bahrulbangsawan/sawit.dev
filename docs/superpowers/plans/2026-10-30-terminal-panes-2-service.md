# Terminal panes 2: the terminal service — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A background process that owns terminal sessions and tab records, and a client library that drives them over a Unix socket with the same surface `TerminalView` uses today.

**Architecture:** `sawit-core::terminal::service` holds the protocol (length-prefixed postcard messages), row-diffed frames, the server (std threads, one `Mutex` of state, a frame pump), and `ServiceClient`/`RemoteSession`. A `TermBackend` trait is implemented by both the in-process `Session` and `RemoteSession`, so plan 3 can swap the app over without touching drawing or input code. The `sawit` binary runs the service when started with `SAWIT_TERMINAL_SERVICE=<socket>`. Nothing in the app uses the service yet.

**Tech Stack:** Rust 2024, `alacritty_terminal` 0.26.0, serde + `postcard` 1 (new dependency, `use-std` feature), `libc` (setsid, signals, kill), std `UnixListener`/`UnixStream`.

**Spec:** `docs/superpowers/specs/2026-10-30-terminal-split-panes-design.md` (sections "The terminal service", "Wire protocol").

**Series:** 2 of 3. Needs plan 1 (`Layout`, `TabState`, `PaneId`, `new_pane_id`, `Session::cwd`).

## Global Constraints

- One service per user; socket in a `0700` directory under `AppPaths.root`; the service refuses peers whose uid differs (`getpeereid`).
- The socket name carries `PROTOCOL: u32 = 1`; a client whose `Hello.protocol` differs gets `Refused`.
- The service exits when it has had no sessions and no clients for the grace period: **10 s** in production (tests pass a shorter one). It never exits while a session is alive, except on SIGTERM / `KillAll`, which end every session first.
- Frames are pushed by the service, only changed rows, at most one per pane per **8 ms**; a full frame on attach and on resize. Status (at-prompt, foreground job, cwd) re-checked every **250 ms** and sent only when changed.
- Dropping a client handle detaches; only `Kill`, `RemoveTab` or `KillAll` end a process.
- Colors are resolved in the service with `Palette::default()`.
- Performance bar: keystroke echo through the service p50 < 5 ms and p99 < 16 ms, measured by Task 7's bench, and no worse than 2× the in-process figure.
- Commits: `feat(terminal): …`, ending with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.

## Review Focus

1. A client that dies mid-message (half a frame written, then EOF): the service drops that client, detaches its panes, clears its tab owners, and keeps serving others — test in Task 4.
2. A program flooding output while a client stops reading: the service never blocks on that client; the client later receives a full frame, and a second client stays responsive — test in Task 4.
3. A stale socket file left by a crashed service: `connect_or_start` removes it and starts a new service — test in Task 6.
4. A pane whose program exited while no one was attached: attaching delivers its last screen and an `Exit` event at once — test in Task 4.
5. Two clients attached to one pane (a tab moving between windows): both get frames; the newest `Resize` wins — test in Task 4.

---

## File Structure

| File | Responsibility |
|---|---|
| `crates/core/src/terminal/backend.rs` (new) | `TermBackend` trait; impl for `Session` |
| `crates/core/src/terminal/service/mod.rs` (new) | module root, `SERVICE_ENV`, re-exports |
| `crates/core/src/terminal/service/protocol.rs` (new) | messages, records, framing |
| `crates/core/src/terminal/service/frame.rs` (new) | `FrameDiff`, `FrameTracker`, `apply` |
| `crates/core/src/terminal/service/server.rs` (new) | `Service`: sessions, tabs, clients, pump |
| `crates/core/src/terminal/service/client.rs` (new) | `ServiceClient`, `RemoteSession`, start-on-demand |
| `crates/core/src/instances.rs` | `AppPaths::terminal_dir`, `terminal_socket` |
| `crates/core/src/terminal/session.rs`, `input.rs`, `palette.rs` | serde derives on wire types |
| `crates/core/tests/terminal_service.rs` (new) | integration tests |
| `crates/core/examples/terminal_service_bench.rs` (new) | latency/throughput bench |
| `crates/app/src/main.rs` | run the service when `SAWIT_TERMINAL_SERVICE` is set |
| `crates/app/tests/terminal_service_binary.rs` (new) | the real binary starts detached and survives its client |

---

### Task 1: `TermBackend` trait

**Files:**
- Create: `crates/core/src/terminal/backend.rs`
- Modify: `crates/core/src/terminal/mod.rs`
- Test: `crates/core/src/terminal/tests.rs`

**Interfaces:**
- Produces:

```rust
/// What a terminal view needs from a session, wherever the session runs.
pub trait TermBackend {
    fn write(&self, bytes: Vec<u8>);
    fn resize(&self, size: GridSize);
    fn size(&self) -> GridSize;
    fn scroll(&self, delta: i32);
    fn scroll_to_bottom(&self);
    fn select_start(&self, col: usize, row: usize, right_half: bool, kind: SelectKind);
    fn select_extend(&self, col: usize, row: usize, right_half: bool);
    fn clear_selection(&self);
    fn selection_text(&self) -> Option<String>;
    fn text_tail(&self, max_lines: usize) -> String;
    fn cd(&self, dir: &Path);
    fn at_prompt(&self) -> bool;
    fn foreground_job(&self) -> Option<u32>;
    fn cwd(&self) -> Option<PathBuf>;
    fn modes(&self) -> Modes;
    fn snapshot(&self, palette: &Palette) -> Snapshot;
    /// Ends the program. Dropping a handle may only detach from it.
    fn kill(&self);
}
```

  `impl TermBackend for Session` delegates to the existing methods; `kill` sends `Msg::Shutdown`.

- [ ] **Step 1: Write the failing test** `session_works_through_the_backend_trait`: spawn `/bin/sh` as `Box<dyn TermBackend>`, `write(b"echo trait-ok\n".to_vec())`, poll `snapshot(&Palette::default()).text()` until it contains `trait-ok`; `kill()`; poll until the event receiver yields `TermEvent::Exit`.
- [ ] **Step 2: Run** `cargo test -p sawit-core session_works_through_the_backend_trait` — Expected: FAIL to compile.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** — Expected: PASS.
- [ ] **Step 5: Commit** `feat(terminal): backend trait over sessions`.

---

### Task 2: Protocol, records and framing

**Files:**
- Create: `crates/core/src/terminal/service/mod.rs`, `crates/core/src/terminal/service/protocol.rs`
- Modify: `crates/core/Cargo.toml` (`postcard = { version = "1", features = ["use-std"] }`), `crates/core/src/terminal/mod.rs` (`pub mod service;`), serde derives on `Command`, `GridSize`, `CellView`, `CursorView`, `SelectKind`, `TermEvent`, `Modes`, `Rgb`
- Test: `crates/core/src/terminal/service/protocol.rs` (`#[cfg(test)] mod tests`)

**Interfaces:**
- Produces (all `#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]`):

```rust
pub const PROTOCOL: u32 = 1;
pub const SERVICE_ENV: &str = "SAWIT_TERMINAL_SERVICE"; // in service/mod.rs
pub const MAX_MESSAGE: u32 = 16 * 1024 * 1024;
pub type TabId = u64;
pub type ReqId = u64;

pub struct Owner { pub client: u64, pub window: u64 }
pub enum PaneKind { Shell, Program, Ssh { key: String, name: String, launch: Command }, Job { key: String, name: String } }
pub struct PaneRecord { pub title: String, pub kind: PaneKind }
pub struct TabRecord {
    pub id: TabId,
    /// Set by the service only (ClaimTab / ReleaseTab / disconnect); ignored in PutTab.
    pub owner: Option<Owner>,
    pub created_unix: u64,
    pub group: Option<PathBuf>,
    pub state: TabState,
    pub panes: BTreeMap<PaneId, PaneRecord>,
}
pub struct PaneStatus { pub at_prompt: bool, pub foreground_job: Option<u32>, pub cwd: Option<PathBuf>, pub exited: bool }

pub enum ClientMsg {
    Hello { protocol: u32 },
    Spawn { req: ReqId, pane: PaneId, cmd: Command, size: GridSize },
    Attach { pane: PaneId },
    Detach { pane: PaneId },
    Kill { pane: PaneId },
    Input { pane: PaneId, bytes: Vec<u8> },
    Resize { pane: PaneId, size: GridSize },
    Scroll { pane: PaneId, delta: i32 },
    ScrollToBottom { pane: PaneId },
    SelectStart { pane: PaneId, col: u16, row: u16, right_half: bool, kind: SelectKind },
    SelectExtend { pane: PaneId, col: u16, row: u16, right_half: bool },
    ClearSelection { pane: PaneId },
    Cd { pane: PaneId, dir: PathBuf },
    SelectionText { req: ReqId, pane: PaneId },
    TextTail { req: ReqId, pane: PaneId, lines: u32 },
    PutTab { tab: TabRecord },
    /// Kills every pane of the tab.
    RemoveTab { tab: TabId },
    ClaimTab { tab: TabId, window: u64 },
    ReleaseTab { tab: TabId },
    ListTabs { req: ReqId },
    KillAll,
}

pub enum ServerMsg {
    Welcome { service_pid: u32, client: u64 },
    Refused { reason: String },
    Spawned { req: ReqId, result: Result<(), String> },
    Frame { pane: PaneId, diff: FrameDiff },
    Status { pane: PaneId, status: PaneStatus },
    /// Title, Bell, Exit, Copy (never Wakeup: frames are the wakeup).
    Event { pane: PaneId, event: TermEvent },
    Text { req: ReqId, text: Option<String> },
    Tabs { req: ReqId, tabs: Vec<TabRecord> },
    /// Another window (maybe of this same client) claimed a tab this client's window had.
    TabTaken { tab: TabId, by: Owner },
}

pub fn write_msg<T: Serialize>(w: &mut impl Write, msg: &T) -> io::Result<()>;   // u32 LE length + postcard bytes
pub fn read_msg<T: DeserializeOwned>(r: &mut impl Read) -> io::Result<T>;        // InvalidData if len > MAX_MESSAGE or decode fails; UnexpectedEof on truncation
```

  (`FrameDiff` comes from Task 3; write Task 2 and 3 in this order but let `protocol.rs` reference `frame::FrameDiff` — implement Task 3's type before running Task 2's tests if needed, or stub it as an empty struct in this task.)

- [ ] **Step 1: Write the failing tests:** `every_message_round_trips` (one value of each `ClientMsg` and `ServerMsg` variant through `write_msg`/`read_msg` into a `Vec<u8>`, `assert_eq!`), `a_truncated_message_is_eof` (drop the last byte → `ErrorKind::UnexpectedEof`), `an_oversized_length_is_rejected` (length prefix `MAX_MESSAGE + 1` → `ErrorKind::InvalidData`, without allocating it).
- [ ] **Step 2: Run** `cargo test -p sawit-core service::protocol` — Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** — Expected: PASS.
- [ ] **Step 5: Commit** `feat(terminal): terminal service protocol`.

---

### Task 3: Row-diffed frames

**Files:**
- Create: `crates/core/src/terminal/service/frame.rs`
- Test: same file, `#[cfg(test)] mod tests`

**Interfaces:**
- Produces:

```rust
pub struct FrameDiff {
    pub cols: u16,
    pub rows: u16,
    /// Every row is present; the receiver replaces its mirror.
    pub full: bool,
    pub lines: Vec<(u16, Vec<CellView>)>,
    pub cursor: Option<(u16, u16)>,
    pub cursor_shape: CursorView,
    pub modes: Modes,
    pub display_offset: u32,
}
#[derive(Default)] pub struct FrameTracker { last: Option<Snapshot> }
impl FrameTracker {
    /// None when nothing changed since the last diff.
    pub fn diff(&mut self, snap: &Snapshot) -> Option<FrameDiff>;
    /// The next diff is full (after attach, or after a dropped frame).
    pub fn reset(&mut self);
}
pub fn apply(mirror: &mut Snapshot, diff: FrameDiff);
pub fn empty_snapshot() -> Snapshot; // 0×0, cursor None, default modes
```

  A diff is full when there is no previous snapshot or `cols`/`rows` changed.

- [ ] **Step 1: Write the failing tests:** `the_first_diff_is_full`, `no_change_means_no_diff`, `one_changed_row_sends_one_row` (change a cell in row 3 → `lines.len() == 1 && lines[0].0 == 3`), `a_resize_sends_a_full_frame`, `cursor_or_modes_alone_still_send_a_diff` (empty `lines`), `applying_diffs_reproduces_the_snapshot` (apply a sequence of three diffs to `empty_snapshot()`; `mirror.lines == last.lines`, cursor and modes equal), `reset_forces_full`. Build snapshots by hand with a `snap(rows: &[&str])` helper.
- [ ] **Step 2: Run** `cargo test -p sawit-core service::frame` — Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** — Expected: PASS.
- [ ] **Step 5: Commit** `feat(terminal): row-diffed terminal frames`.

---

### Task 4: The service

**Files:**
- Create: `crates/core/src/terminal/service/server.rs`
- Modify: `crates/core/src/instances.rs` (paths)
- Test: `crates/core/tests/terminal_service.rs`, `crates/core/src/instances.rs` tests

**Interfaces:**
- Consumes: Tasks 1–3; plan 1's `TabState`.
- Produces:
  - `impl AppPaths { pub fn terminal_dir(&self) -> PathBuf; pub fn terminal_socket(&self, protocol: u32) -> PathBuf }` — `root/terminal/v{protocol}.sock`, created `0700`, with the same under-100-bytes fallback `cli_socket` uses.
  - `pub fn pidfile(socket: &Path) -> PathBuf` (`socket.with_extension("pid")`).
  - `pub struct Service` with:
    - `pub fn bind(socket: &Path) -> io::Result<Service>` — removes a stale socket only if nothing answers on it; writes the pidfile.
    - `pub fn stopper(&self) -> Stopper` where `Stopper::stop(&self)` makes `run` end every session and return (`#[derive(Clone)]`, an `Arc<AtomicBool>`).
    - `pub fn run(self, grace: Duration) -> io::Result<()>` — accept loop (non-blocking, 50 ms poll so `stop` and grace are seen), removes socket and pidfile on return.
  - `pub fn run_service(socket: &Path) -> std::process::ExitCode` — `bind`, installs a SIGTERM/SIGHUP handler (`libc::signal`, sets the stopper's flag; SIGHUP is ignored so closing the launching terminal is harmless, SIGTERM stops), `run(Duration::from_secs(10))`.
  - Behaviour (per client: one reader thread, one writer thread fed by `sync_channel(256)`):
    - `Hello` must come first; other protocol → `Refused { reason: "Sawit's terminal service speaks protocol {PROTOCOL}." }` and close.
    - `Spawn` → `Session::spawn` (cmd as given) → `Spawned`; a thread per session drains its events: `Wakeup` marks the pane dirty, others are forwarded as `Event` to attached clients; `Exit` marks `status.exited`.
    - `Attach` → new `FrameTracker` for (pane, client), full frame now, current `Status`, and an `Event{Exit}` if exited.
    - Pump thread: every 8 ms, for each dirty pane, one `snapshot(&Palette::default())`, then `diff` per attached client. Frames go through `try_send`; a full queue drops the frame and `reset`s that tracker. A full queue for any non-frame message disconnects that client.
    - Every 250 ms, `Status` for panes whose `PaneStatus` changed.
    - `Kill` drops the session (its process gets SIGHUP from the closed PTY) and removes the pane from any tab record's `panes` map; `RemoveTab` kills all its panes and the record; `KillAll` kills everything.
    - `PutTab` stores the record keeping the service's `owner`; `ClaimTab` sets `owner = Owner { client, window }` and, when the previous owner differs (client or window), sends `TabTaken { tab, by: new owner }` to the previous owner's client; `ReleaseTab` clears it; `ListTabs` replies with every record.
    - Client EOF or read error: detach its panes, clear `owner` on its tabs, end its threads.
    - Grace: with no sessions and no clients continuously for `grace`, `run` returns.

- [ ] **Step 1: Write the failing integration tests** in `crates/core/tests/terminal_service.rs`, with helpers `start(grace) -> (TempDir, PathBuf socket, Stopper, JoinHandle)` and a raw test client `Raw { stream }` with `send(ClientMsg)` and `recv_until(|m: &ServerMsg| -> bool, timeout) -> ServerMsg` that applies `Frame`s to a mirror `Snapshot` it keeps:

```rust
#[test] fn hello_with_another_protocol_is_refused()
#[test] fn output_reaches_an_attached_client()            // Spawn sh, Attach, Input "echo svc-ok\n", mirror text contains "svc-ok"
#[test] fn a_session_outlives_its_client()                // client A spawns + PutTab + ClaimTab; drop A; client B ListTabs → owner None; Attach → first full frame text contains A's output
#[test] fn a_client_dying_mid_message_is_dropped()        // write 4-byte length 1000 + 10 bytes, close; another client still gets ListTabs replies; the tab's owner is cleared
#[test] fn a_slow_client_does_not_stall_others()          // client A attaches a pane running `yes`, never reads for 2 s; client B's Input echo arrives within 500 ms; then A reads and gets a full frame
#[test] fn attaching_to_an_exited_pane_reports_exit()     // Spawn `sh -c "echo bye"`, wait 500 ms unattached, Attach → frame text contains "bye" and Event{Exit}
#[test] fn two_clients_share_a_pane()                     // both Attach; Input → both mirrors show it; B Resize to 100 cols → both get full frames with cols 100
#[test] fn claiming_a_tab_tells_the_previous_owner()      // A claims (window 1); B claims → A receives TabTaken{by.client == B}; A claims with window 2 → B gets TabTaken; A claims window 2 again → nothing sent
#[test] fn kill_ends_the_process()                        // Spawn sh, read its pid via Input "echo $$\n"; Kill → Event{Exit}; libc::kill(pid, 0) == -1 within 2 s
#[test] fn remove_tab_kills_its_panes()
#[test] fn the_service_exits_after_the_grace_period()     // grace 300 ms; no clients; JoinHandle finishes within 2 s; socket and pidfile gone
#[test] fn the_service_waits_while_a_session_lives()      // grace 300 ms; spawn sh; disconnect; after 1 s the thread is still running; stopper.stop() → it ends and the process is gone
```

  Plus unit test in `instances.rs`: `terminal_socket_stays_under_the_unix_path_limit` (mirrors `cli_socket_stays_under_the_unix_path_limit`).

- [ ] **Step 2: Run** `cargo test -p sawit-core --test terminal_service` — Expected: FAIL to compile.
- [ ] **Step 3: Implement** `server.rs` and the paths.
- [ ] **Step 4: Run** `cargo test -p sawit-core --test terminal_service` and `cargo test -p sawit-core instances` — Expected: PASS (run the service tests 5× with `-- --test-threads=1` once to catch flakiness: `for i in 1 2 3 4 5; do cargo test -p sawit-core --test terminal_service || break; done`).
- [ ] **Step 5: Commit** `feat(terminal): background terminal service`.

---

### Task 5: Client and `RemoteSession`

**Files:**
- Create: `crates/core/src/terminal/service/client.rs`
- Test: `crates/core/tests/terminal_service.rs`

**Interfaces:**
- Produces:

```rust
#[derive(Clone)] pub struct ServiceClient { /* Arc inner: writer Mutex<UnixStream>, pending replies, per-pane mirrors/status/event senders, client id */ }
pub enum ClientNotice { TabTaken { tab: TabId, by: Owner }, Disconnected }

impl ServiceClient {
    pub fn connect(socket: &Path) -> io::Result<ServiceClient>;            // Hello; Refused → io::Error(Other, reason)
    pub fn id(&self) -> u64;
    pub fn spawn(&self, pane: PaneId, cmd: Command, size: GridSize) -> Result<(RemoteSession, mpsc::UnboundedReceiver<TermEvent>)>;
    pub fn attach(&self, pane: PaneId) -> (RemoteSession, mpsc::UnboundedReceiver<TermEvent>);
    pub fn put_tab(&self, tab: TabRecord);
    pub fn remove_tab(&self, tab: TabId);
    pub fn claim_tab(&self, tab: TabId, window: u64);
    pub fn release_tab(&self, tab: TabId);
    pub fn list_tabs(&self) -> io::Result<Vec<TabRecord>>;
    pub fn kill_all(&self);
    /// TabTaken and Disconnected, in order.
    pub fn notices(&self) -> mpsc::UnboundedReceiver<ClientNotice>;     // one receiver; a second call returns an already-closed one
}

pub struct RemoteSession { pane: PaneId, client: ServiceClient, mirror: Arc<Mutex<Snapshot>>, status: Arc<Mutex<PaneStatus>>, size: Mutex<GridSize> }
impl RemoteSession { pub fn pane(&self) -> PaneId }
impl TermBackend for RemoteSession { … }   // write/resize/scroll/select/cd: fire-and-forget; selection_text/text_tail: request, 1 s timeout (None / "" on timeout); at_prompt/foreground_job/cwd: from the last Status; snapshot: mirror clone (palette ignored, colors are resolved by the service); kill: Kill message
impl Drop for RemoteSession { fn drop(&mut self) { /* Detach only */ } }
```

  `mpsc` is `futures::channel::mpsc` (what `TerminalView` already consumes). The reader thread turns each `Frame` into an `apply` on that pane's mirror plus a `TermEvent::Wakeup`, forwards `Event`s, stores `Status`, completes pending requests; on EOF it sends `TermEvent::Exit` to every pane, sets their `status.exited`, and sends `ClientNotice::Disconnected`. `resize` skips sending when the size is unchanged (as `Session::resize` does).

- [ ] **Step 1: Write the failing tests:**

```rust
#[test] fn remote_session_echoes_and_reads_scrollback()   // spawn sh; write "echo remote-ok\n"; poll snapshot text; text_tail(50) contains "remote-ok"
#[test] fn remote_selection_copies_text()                 // after echo, select_start(0,row) / select_extend(8,row) on the echoed row → selection_text() starts with "remote-o"
#[test] fn dropping_a_remote_session_only_detaches()      // drop RemoteSession; new client attach(pane) shows earlier output; process alive
#[test] fn cwd_and_prompt_status_arrive()                 // spawn sh in tmp dir; poll cwd() == tmp and at_prompt() == true
#[test] fn a_dead_service_ends_every_pane()               // stopper.stop(); receiver yields Exit; notices() yields Disconnected
```

- [ ] **Step 2: Run** `cargo test -p sawit-core --test terminal_service` — Expected: new tests FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** — Expected: PASS.
- [ ] **Step 5: Commit** `feat(terminal): terminal service client`.

---

### Task 6: Starting the service from the `sawit` binary

**Files:**
- Modify: `crates/core/src/terminal/service/client.rs`, `crates/app/src/main.rs`
- Create: `crates/app/tests/terminal_service_binary.rs`
- Test: that file, plus `crates/core/tests/terminal_service.rs`

**Interfaces:**
- Produces:
  - `impl ServiceClient { pub fn connect_or_start(socket: &Path, exe: &Path) -> io::Result<ServiceClient> }` — try `connect`; on `NotFound`/`ConnectionRefused`, remove a stale socket file, spawn `exe` with `SERVICE_ENV=<socket>`, `pre_exec(|| { libc::setsid(); Ok(()) })`, stdin null, stdout+stderr appended to `terminal_dir/service.log`; retry `connect` every 20 ms for up to 3 s.
  - `pub fn older_services(dir: &Path) -> Vec<(u32, u32)>` — `(protocol, pid)` for each `v{N}.pid` with `N < PROTOCOL` whose pid is alive (`kill(pid, 0) == 0`); `pub fn end_service(pid: u32)` — SIGTERM.
  - `main.rs`: first thing in `main` after logger setup: if `std::env::var_os(SERVICE_ENV)` is set, `unsafe { std::env::remove_var(SERVICE_ENV) }` (single-threaded here) and `return sawit_core::terminal::service::run_service(Path::new(&socket))`.

- [ ] **Step 1: Write the failing tests:**

```rust
// crates/core/tests/terminal_service.rs
#[test] fn a_stale_socket_file_is_replaced()      // bind a UnixListener at the socket path and drop it (file left, nobody listening);
                                                  // Service::bind on that path succeeds and clients can connect
#[test] fn older_services_lists_live_pids_only()  // write v0.pid with std::process::id() and another with pid 999_999 → only the live one

// crates/app/tests/terminal_service_binary.rs (uses env!("CARGO_BIN_EXE_sawit"))
#[test] fn the_binary_runs_a_detached_service_that_outlives_its_client() {
    // SAWIT_HOME = temp dir; socket = AppPaths::resolve(Some(temp), None).terminal_socket(PROTOCOL)
    // a = connect_or_start(socket, exe); spawn sh; put_tab; drop a;
    // b = ServiceClient::connect(socket) succeeds; list_tabs has the tab; attach → output still there;
    // b.kill_all(); the service exits (socket gone within 3 s).
}

#[test]
fn a_stale_socket_does_not_stop_a_new_start() {
    // leave a dead socket file at the path (bind + drop a UnixListener); connect_or_start succeeds.
}
```

- [ ] **Step 2: Run** `cargo test -p sawit-core --test terminal_service` and `cargo test -p sawit --test terminal_service_binary` — Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** both — Expected: PASS. Also `ps -o pid,ppid,sess,command -p <service pid>` during a manual run shows it in its own session.
- [ ] **Step 5: Commit** `feat(terminal): start the terminal service on demand`.

---

### Task 7: Latency and throughput bench

**Files:**
- Create: `crates/core/examples/terminal_service_bench.rs`

**Interfaces:**
- Consumes: `Service`, `ServiceClient`, `Session`.
- Prints, for in-process `Session` and for `RemoteSession` through an in-thread service: keystroke echo latency p50/p99 over 500 keystrokes to `cat` (time from `write` to the byte appearing in `snapshot`), and frames/s plus MB/s while `/bin/sh -c 'while :; do seq 1 2000; done'` runs in a 250×60 pane for 5 s.

- [ ] **Step 1: Write the bench.**
- [ ] **Step 2: Run** `cargo run -p sawit-core --release --example terminal_service_bench` — Expected: remote echo p50 < 5 ms, p99 < 16 ms, and each no worse than 2× in-process. If not, profile the pump (encode cost, per-client diff) and fix before continuing; record the numbers in the commit message.
- [ ] **Step 3: Commit** `test(terminal): bench the terminal service` with the measured numbers in the body.
