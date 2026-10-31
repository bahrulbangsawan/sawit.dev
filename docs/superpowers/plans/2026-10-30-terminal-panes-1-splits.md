# Terminal panes 1: split panes and herdr removal — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Terminal tabs in the dock hold split panes with herdr's pane menu, all in-process, and herdr is removed from Sawit.

**Architecture:** A pure layout model (`Layout`, `TabState`) in `sawit-core::terminal` decides splits, focus, zoom and pane rectangles. A new `TerminalTab` entity in the app renders one tab's panes (headers, dividers, one `TerminalView` per pane) and replaces `Entity<TerminalView>` as the unit of `TerminalPanel.tabs`. Sessions stay in-process in this plan; plans 2 and 3 move them into a background service.

**Tech Stack:** Rust 2024, GPUI via `gpui-kit` 0.8.1, `alacritty_terminal` 0.26.0, `libc` (macOS `proc_pidinfo`), serde.

**Spec:** `docs/superpowers/specs/2026-10-30-terminal-split-panes-design.md`

**Series:** 1 of 3. Plan 2: `2026-10-30-terminal-panes-2-service.md`. Plan 3: `2026-10-30-terminal-panes-3-background.md`.

## Global Constraints

- `alacritty_terminal` stays pinned at `=0.26.0`; GPUI only through `gpui-kit` `=0.8.1` (API notes: `docs/notes/gpui-kit-0.8.1-api.md`).
- Pane menu copy, in this order: "Rename pane", "Split right", "Split down", "Zoom" (reads "Unzoom" when zoomed), "Send right-clicks to pane" (checkmark), "Close pane".
- Panes never shrink below 20 columns or 4 rows.
- Pane headers appear only when the tab has more than one pane.
- Keys, Terminal key context only: `cmd-d` Split right, `cmd-shift-d` Split down, `cmd-alt-left/right/up/down` focus neighbour, `cmd-shift-enter` Zoom when the tab has >1 pane, otherwise it propagates to the workspace (maximize dock). No new close shortcut.
- Every new Terminal-context binding goes in `terminal::view::bindings()` so the existing tests `no_binding_takes_a_shell_key_from_the_terminal` and `app_shortcuts_still_work_from_the_terminal` (`crates/app/src/terminal/view.rs`) keep passing unchanged.
- Shift+right-click always opens the pane menu.
- Code style: match surrounding code (doc comments on every pub item, short sentences, no `unwrap` outside tests).
- Commits: `feat(terminal): …` / `chore(terminal): …`, ending with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.

## Review Focus

1. Splitting a pane too small to halve (would drop below 20 cols / 4 rows): the split is refused and nothing changes — test in Task 2.
2. The focused pane's shell sits in a folder that no longer exists (or `cwd()` returns `None`): the new pane starts in the tab's project folder, then `$HOME` — test in Task 5.
3. Closing the zoomed pane: zoom clears and focus goes to the sibling — test in Task 2.
4. The dock dragged very short / narrow with several panes: `rects` never returns negative or NaN sizes — test in Task 1.
5. Renaming to whitespace only: treated as empty, the program's title comes back — test in Task 2.

---

## File Structure

| File | Responsibility |
|---|---|
| `crates/core/src/terminal/layout.rs` (new) | `Layout` split tree, rectangles, dividers, neighbours, ratios |
| `crates/core/src/terminal/tab.rs` (new) | `TabState`: panes' metadata, focus, zoom, split/close rules |
| `crates/core/src/terminal/session.rs` | add `Session::cwd()` |
| `crates/core/src/terminal/mod.rs` | exports |
| `crates/core/src/terminal/tests.rs` | layout/tab/cwd tests (existing test module) |
| `crates/app/src/terminal/tab.rs` (new) | `TerminalTab` entity: renders panes, headers, dividers, pane menu, rename |
| `crates/app/src/terminal/panel.rs` | tabs become `Entity<TerminalTab>`; spawning for splits; herdr removed |
| `crates/app/src/terminal/view.rs` | right-click routing, new bindings and actions |
| `crates/app/src/terminal/mod.rs` | module + actions |
| `crates/app/src/workspace/control.rs`, `crates/mcp/src/app.rs` | `pane` argument and pane listing |
| herdr files (Task 9) | removal |

---

### Task 1: Layout tree

**Files:**
- Create: `crates/core/src/terminal/layout.rs`
- Modify: `crates/core/src/terminal/mod.rs` (add `mod layout; pub use layout::*` items below)
- Test: `crates/core/src/terminal/tests.rs`

**Interfaces:**
- Produces:
  - `pub type PaneId = u64;` and `pub fn new_pane_id() -> PaneId` (random, from `std::hash::RandomState` hashing an `AtomicU64` counter; ids must be unique across processes in practice, since plan 3 keys a shared service by them).
  - `#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)] pub enum SplitDir { Right, Down }`
  - `#[derive(Clone, Copy, Debug, PartialEq, Eq)] pub enum Direction { Left, Right, Up, Down }`
  - `#[derive(Clone, Copy, Debug, PartialEq, Default)] pub struct Rect { pub x: f32, pub y: f32, pub w: f32, pub h: f32 }`
  - `#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)] pub enum Layout { Pane(PaneId), Split { dir: SplitDir, ratio: f32, first: Box<Layout>, second: Box<Layout> } }`
  - `#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)] pub struct SplitPath(pub Vec<bool>)` — `false` = first child, `true` = second.
  - `pub struct Divider { pub path: SplitPath, pub dir: SplitDir, pub at: f32, pub span: Rect }` (`at` is the x of a `Right` split's boundary or the y of a `Down` split's; `span` is the split's whole area).
  - `impl Layout`:
    - `pub fn panes(&self) -> Vec<PaneId>` — reading order (first before second).
    - `pub fn split(&mut self, target: PaneId, dir: SplitDir, new: PaneId) -> bool` — replaces `Pane(target)` with `Split { dir, ratio: 0.5, first: target, second: new }`; false if `target` is absent.
    - `pub fn remove(&mut self, target: PaneId) -> Removed` with `pub enum Removed { NotFound, Last, Focus(PaneId) }`; the sibling subtree takes the split's place; `Focus` is the sibling subtree's first pane.
    - `pub fn rects(&self, area: Rect) -> Vec<(PaneId, Rect)>` — widths/heights `max(0.0)`, never NaN.
    - `pub fn dividers(&self, area: Rect) -> Vec<Divider>`
    - `pub fn neighbor(&self, from: PaneId, dir: Direction, area: Rect) -> Option<PaneId>` — among panes whose rect lies fully on that side of `from`'s rect and overlaps it on the other axis, the closest edge; ties go to the one overlapping `from`'s centre line most.
    - `pub fn set_ratio(&mut self, path: &SplitPath, ratio: f32, span_len: f32, min_len: f32)` — clamps `ratio` to `[min_len/span_len, 1 - min_len/span_len]` (to 0.5 if `span_len < 2*min_len`).
    - `pub fn even(&mut self, path: &SplitPath)` — ratio 0.5.
- Add `serde` derives (core already depends on `serde` with `derive`).

- [ ] **Step 1: Write the failing tests** in `tests.rs`:

```rust
#[test]
fn split_right_puts_the_new_pane_after_the_target() {
    let mut l = Layout::Pane(1);
    assert!(l.split(1, SplitDir::Right, 2));
    assert!(l.split(1, SplitDir::Down, 3));
    assert_eq!(l.panes(), vec![1, 3, 2]);
    assert!(!l.split(9, SplitDir::Right, 4));
}

#[test]
fn rects_divide_the_area_by_ratio() {
    let mut l = Layout::Pane(1);
    l.split(1, SplitDir::Right, 2);
    l.split(2, SplitDir::Down, 3);
    let r = l.rects(Rect { x: 0., y: 0., w: 800., h: 400. });
    assert_eq!(r[0], (1, Rect { x: 0., y: 0., w: 400., h: 400. }));
    assert_eq!(r[1], (2, Rect { x: 400., y: 0., w: 400., h: 200. }));
    assert_eq!(r[2], (3, Rect { x: 400., y: 200., w: 400., h: 200. }));
}

#[test]
fn rects_never_go_negative_in_a_tiny_area() {
    let mut l = Layout::Pane(1);
    for n in 2..8 { l.split(n - 1, SplitDir::Right, n); }
    for (_, r) in l.rects(Rect { x: 0., y: 0., w: 3., h: 0. }) {
        assert!(r.w >= 0. && r.h >= 0. && !r.w.is_nan() && !r.h.is_nan());
    }
}

#[test]
fn removing_a_pane_gives_its_space_to_the_sibling() {
    let mut l = Layout::Pane(1);
    l.split(1, SplitDir::Right, 2);
    l.split(2, SplitDir::Down, 3);
    assert_eq!(l.remove(1), Removed::Focus(2));
    assert_eq!(l.panes(), vec![2, 3]);
    assert_eq!(l.remove(3), Removed::Focus(2));
    assert_eq!(l, Layout::Pane(2));
    assert_eq!(l.remove(2), Removed::Last);
    assert_eq!(l.remove(7), Removed::NotFound);
}

#[test]
fn neighbor_follows_the_direction() {
    // 1 | 2
    //   | 3
    let mut l = Layout::Pane(1);
    l.split(1, SplitDir::Right, 2);
    l.split(2, SplitDir::Down, 3);
    let a = Rect { x: 0., y: 0., w: 800., h: 400. };
    assert_eq!(l.neighbor(1, Direction::Right, a), Some(2));
    assert_eq!(l.neighbor(3, Direction::Left, a), Some(1));
    assert_eq!(l.neighbor(2, Direction::Down, a), Some(3));
    assert_eq!(l.neighbor(3, Direction::Up, a), Some(2));
    assert_eq!(l.neighbor(1, Direction::Left, a), None);
}

#[test]
fn set_ratio_keeps_both_sides_at_least_min() {
    let mut l = Layout::Pane(1);
    l.split(1, SplitDir::Right, 2);
    let root = SplitPath(vec![]);
    l.set_ratio(&root, 0.05, 800., 160.);
    assert!(matches!(l, Layout::Split { ratio, .. } if (ratio - 0.2).abs() < 1e-6));
    l.set_ratio(&root, 0.99, 800., 160.);
    assert!(matches!(l, Layout::Split { ratio, .. } if (ratio - 0.8).abs() < 1e-6));
    l.even(&root);
    assert!(matches!(l, Layout::Split { ratio, .. } if ratio == 0.5));
    assert_eq!(l.dividers(Rect { x: 0., y: 0., w: 800., h: 400. })[0].at, 400.);
}

#[test]
fn pane_ids_are_unique() {
    let ids: std::collections::HashSet<_> = (0..1000).map(|_| new_pane_id()).collect();
    assert_eq!(ids.len(), 1000);
}
```

- [ ] **Step 2: Run** `cargo test -p sawit-core terminal::tests` — Expected: FAIL to compile (`Layout` not found).
- [ ] **Step 3: Implement** `layout.rs` per the Interfaces block.
- [ ] **Step 4: Run** `cargo test -p sawit-core terminal::tests` — Expected: PASS.
- [ ] **Step 5: Commit** `feat(terminal): split layout model for panes`.

---

### Task 2: Tab state (focus, zoom, names, split and close rules)

**Files:**
- Create: `crates/core/src/terminal/tab.rs`
- Modify: `crates/core/src/terminal/mod.rs`
- Test: `crates/core/src/terminal/tests.rs`

**Interfaces:**
- Consumes: Task 1 (`Layout`, `PaneId`, `SplitDir`, `Direction`, `Rect`, `Removed`).
- Produces:
  - `#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)] pub struct PaneMeta { pub name: Option<String>, pub send_right_clicks: bool }`
  - `#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)] pub struct TabState { pub layout: Layout, pub focused: PaneId, pub zoomed: Option<PaneId>, pub panes: BTreeMap<PaneId, PaneMeta> }`
  - `pub const MIN_COLS: u16 = 20; pub const MIN_ROWS: u16 = 4;`
  - `impl TabState`:
    - `pub fn new(first: PaneId) -> Self`
    - `pub fn split(&mut self, dir: SplitDir, new: PaneId, area: Rect, min: (f32, f32)) -> bool` — splits `focused`; refused (false, no change) when the focused pane's rect halved along `dir` would be narrower than `min.0` (Right) or shorter than `min.1` (Down). On success: adds `PaneMeta::default()`, focuses `new`, clears `zoomed`.
    - `pub fn close(&mut self, pane: PaneId) -> Removed` — removes from layout and `panes`; clears `zoomed` if it was `pane`; on `Focus(p)` sets `focused = p` only if `pane` was focused.
    - `pub fn focus(&mut self, pane: PaneId)` — clears `zoomed` when it is another pane.
    - `pub fn focus_dir(&mut self, dir: Direction, area: Rect) -> bool`
    - `pub fn toggle_zoom(&mut self) -> bool` — false (no change) with one pane.
    - `pub fn rename(&mut self, pane: PaneId, name: &str)` — trimmed empty → `None`.
    - `pub fn toggle_right_clicks(&mut self, pane: PaneId)`
    - `pub fn visible(&self, area: Rect) -> Vec<(PaneId, Rect)>` — only `(zoomed, area)` when zoomed, else `layout.rects(area)`.
    - `pub fn is_split(&self) -> bool`

- [ ] **Step 1: Write the failing tests:**

```rust
fn area() -> Rect { Rect { x: 0., y: 0., w: 800., h: 400. } }
const MIN: (f32, f32) = (160., 80.);

#[test]
fn split_focuses_the_new_pane_and_unzooms() {
    let mut t = TabState::new(1);
    assert!(t.split(SplitDir::Right, 2, area(), MIN));
    assert_eq!(t.focused, 2);
    assert!(t.toggle_zoom());
    assert!(t.split(SplitDir::Down, 3, area(), MIN));
    assert_eq!(t.zoomed, None);
    assert_eq!(t.panes.len(), 3);
}

#[test]
fn split_is_refused_when_the_pane_is_too_small() {
    let mut t = TabState::new(1);
    let small = Rect { x: 0., y: 0., w: 300., h: 400. };
    assert!(!t.split(SplitDir::Right, 2, small, MIN));
    assert_eq!(t.layout, Layout::Pane(1));
    assert_eq!(t.panes.len(), 1);
}

#[test]
fn closing_the_zoomed_pane_unzooms_and_focuses_the_sibling() {
    let mut t = TabState::new(1);
    t.split(SplitDir::Right, 2, area(), MIN);
    t.toggle_zoom();
    assert_eq!(t.close(2), Removed::Focus(1));
    assert_eq!((t.focused, t.zoomed), (1, None));
    assert_eq!(t.close(1), Removed::Last);
}

#[test]
fn zoom_needs_two_panes_and_shows_only_the_zoomed_one() {
    let mut t = TabState::new(1);
    assert!(!t.toggle_zoom());
    t.split(SplitDir::Right, 2, area(), MIN);
    t.toggle_zoom();
    assert_eq!(t.visible(area()), vec![(2, area())]);
    t.focus(1);
    assert_eq!(t.zoomed, None);
}

#[test]
fn rename_trims_and_clears() {
    let mut t = TabState::new(1);
    t.rename(1, "  api  ");
    assert_eq!(t.panes[&1].name.as_deref(), Some("api"));
    t.rename(1, "   ");
    assert_eq!(t.panes[&1].name, None);
    t.toggle_right_clicks(1);
    assert!(t.panes[&1].send_right_clicks);
}
```

- [ ] **Step 2: Run** `cargo test -p sawit-core terminal::tests` — Expected: FAIL (no `TabState`).
- [ ] **Step 3: Implement** `tab.rs` per Interfaces.
- [ ] **Step 4: Run** the same — Expected: PASS.
- [ ] **Step 5: Commit** `feat(terminal): tab state with focus, zoom and pane names`.

---

### Task 3: A session's working folder

**Files:**
- Modify: `crates/core/src/terminal/session.rs`
- Test: `crates/core/src/terminal/tests.rs`

**Interfaces:**
- Produces: `impl Session { pub fn cwd(&self) -> Option<PathBuf> }` — the current folder of the program started in the PTY (the shell, by the pid already kept in `foreground`), via `libc::proc_pidinfo(pid, libc::PROC_PIDVNODEPATHINFO, …)` into `libc::proc_vnodepathinfo` (`pvi_cdir.vip_path`) on macOS; `None` elsewhere or on error.

- [ ] **Step 1: Write the failing test:**

```rust
#[cfg(target_os = "macos")]
#[test]
fn cwd_follows_the_shell() {
    let dir = tempfile::tempdir().unwrap();
    let dir = dir.path().canonicalize().unwrap();
    let cmd = Command { program: "/bin/sh".into(), cwd: Some(dir.clone()), ..Command::default() };
    let (s, _rx) = Session::spawn(cmd, GridSize { cols: 80, rows: 24, cell_width: 8, cell_height: 17 }).unwrap();
    wait_until(|| s.cwd().as_deref() == Some(dir.as_path()));
    s.write(b"cd /\n".to_vec());
    wait_until(|| s.cwd().as_deref() == Some(std::path::Path::new("/")));
}
```

(Add `fn wait_until(ok: impl Fn() -> bool)` next to the existing `wait_for` helper in `tests.rs`: polls every 20 ms for up to 5 s, panics on timeout.)

- [ ] **Step 2: Run** `cargo test -p sawit-core cwd_follows_the_shell` — Expected: FAIL (no `cwd`).
- [ ] **Step 3: Implement** `Session::cwd`.
- [ ] **Step 4: Run** — Expected: PASS.
- [ ] **Step 5: Commit** `feat(terminal): read a session's working folder`.

---

### Task 4: `TerminalTab` replaces a tab's single view (no behaviour change)

**Files:**
- Create: `crates/app/src/terminal/tab.rs`
- Modify: `crates/app/src/terminal/panel.rs`, `crates/app/src/terminal/mod.rs`, `crates/app/src/terminal/view.rs` (move `group` off the view)
- Test: `crates/app/src/terminal/tab.rs` (`#[cfg(test)] mod tests`)

**Interfaces:**
- Consumes: Task 2 `TabState`, `PaneId`, `new_pane_id`.
- Produces:
  - `pub struct TerminalTab { pub state: TabState, views: HashMap<PaneId, Entity<TerminalView>>, pub group: Option<PathBuf>, bounds: Rc<Cell<Option<Bounds<Pixels>>>>, … }`
  - `impl TerminalTab`:
    - `pub fn new(view: Entity<TerminalView>, group: Option<PathBuf>, cx: &mut Context<Self>) -> Self`
    - `pub fn focused_view(&self) -> &Entity<TerminalView>`
    - `pub fn views(&self) -> impl Iterator<Item = (PaneId, &Entity<TerminalView>)>` — layout order
    - `pub fn title(&self, cx: &App) -> SharedString` — focused pane's `PaneMeta.name`, else its view's title
    - `pub fn focus(&self, window: &mut Window, cx: &mut App)` — focuses the focused pane's view
    - `pub fn has_focus(&self, window: &Window, cx: &App) -> bool`
  - `pub enum TabEvent { Split(SplitDir), Changed, Closed }` with `impl EventEmitter<TabEvent> for TerminalTab` (`Closed` = its last pane closed).
  - `TerminalPanel.tabs: Vec<Entity<TerminalTab>>`. Every existing method keeps its signature; methods that read `view.remote`/`view.job`/`view.exited` read the tab's **focused** view in this task (Task 8 widens them to all panes). `view.group` moves to `TerminalTab.group`.
- Rendering in this task: a tab renders its single pane full size (Task 5 adds layout).

- [ ] **Step 1: Write the failing GPUI test** (open a `TerminalPanel` in a headless window like `view.rs`'s tests do):

```rust
#[gpui_kit::test]
fn a_new_panel_has_one_tab_of_one_pane(cx: &mut TestAppContext) {
    cx.update(init);
    let (_w, panel) = open_panel(cx); // helper: open_window with cx.new(TerminalPanel::new)
    cx.update(|cx| {
        let p = panel.read(cx);
        assert_eq!(p.tab_count(), 1);
        let tab = p.tab(0).read(cx);
        assert_eq!(tab.views().count(), 1);
        assert!(!tab.state.is_split());
    });
}
```

Add `pub fn tab_count(&self) -> usize` and `pub fn tab(&self, ix: usize) -> &Entity<TerminalTab>` to `TerminalPanel` (also used by later tests).

- [ ] **Step 2: Run** `cargo test -p sawit terminal::` — Expected: FAIL to compile.
- [ ] **Step 3: Implement** the refactor. The panel subscribes to each tab's `TabEvent` (`Closed` → remove that tab; `Changed` → `cx.notify()`).
- [ ] **Step 4: Run** `cargo test -p sawit terminal::` and `cargo clippy -p sawit -- -D warnings` — Expected: PASS, no warnings. Manually: `cargo run -p sawit`, open/close terminals, SSH tab, Run script — behaviour unchanged.
- [ ] **Step 5: Commit** `refactor(terminal): tabs hold panes`.

---

### Task 5: Splitting, focus moves, dividers and headers

**Files:**
- Modify: `crates/app/src/terminal/tab.rs`, `crates/app/src/terminal/panel.rs`, `crates/app/src/terminal/view.rs` (`bindings()`), `crates/app/src/terminal/mod.rs` (actions)
- Test: `crates/app/src/terminal/tab.rs`

**Interfaces:**
- Consumes: Tasks 1–4; `Session::cwd` via a new `TerminalView::cwd(&self) -> Option<PathBuf>`.
- Produces:
  - Actions in `terminal` namespace: `SplitRight`, `SplitDown`, `FocusLeft`, `FocusRight`, `FocusUp`, `FocusDown` (`actions!(terminal, [...])`), bound in `view::bindings()` with context `"Terminal"` to `cmd-d`, `cmd-shift-d`, `cmd-alt-left`, `cmd-alt-right`, `cmd-alt-up`, `cmd-alt-down`.
  - `impl TerminalTab { pub fn add_pane(&mut self, dir: SplitDir, view: Entity<TerminalView>, window: &mut Window, cx: &mut Context<Self>) -> bool }` — calls `state.split` with the last painted bounds and `min = (MIN_COLS * cell_w, MIN_ROWS * cell_h + HEADER_H)`; on success stores and focuses the view. Refused → drops the view (its process ends).
  - `TerminalPanel::split_active(&mut self, dir: SplitDir, window: &mut Window, cx: &mut Context<Self>)` — spawns a shell whose cwd is the focused view's `cwd()` if that folder still exists, else the tab's `group`, else `$HOME` (same fallback as `spawn`), then `add_pane`.
  - `const HEADER_H: Pixels = px(24.)`.
- Render rules (from the spec): panes absolutely positioned from `state.visible(bounds)` (bounds recorded by a `canvas` prepaint as `view.rs` does; first frame with no bounds renders the focused pane full size). With `is_split()`: each pane has a header (icon — `Lucide::Server` for SSH, `Lucide::Play` for a script, else `Lucide::Terminal`; title; folder with `$HOME` shown as `~`; ghost xsmall buttons Split right, Zoom/Unzoom, Close) and a 1 px border, the focused pane's border in the theme's focus ring color, others `cx.theme().border`. Dividers: 5 px hit strips centred on each `Divider.at`, resize cursor, drag updates `Layout::set_ratio` with the same mins (window-level mouse move/up listeners, as the view's selection drag does), double-click calls `even`. Clicking a pane calls `state.focus`.

- [ ] **Step 1: Write the failing GPUI tests:**

```rust
#[gpui_kit::test]
fn cmd_d_splits_right_and_focuses_the_new_pane(cx: &mut TestAppContext) {
    cx.update(init);
    let (w, panel) = open_panel(cx);
    cx.simulate_keystrokes(w, "cmd-d");
    cx.run_until_parked();
    cx.update(|cx| {
        let tab = panel.read(cx).tab(0).read(cx);
        assert_eq!(tab.views().count(), 2);
        assert!(matches!(tab.state.layout, Layout::Split { dir: SplitDir::Right, .. }));
        assert_eq!(tab.state.focused, tab.state.layout.panes()[1]);
    });
}

#[gpui_kit::test]
fn cmd_shift_d_splits_down_and_cmd_alt_arrows_move_focus(cx: &mut TestAppContext) {
    cx.update(init);
    let (w, panel) = open_panel(cx);
    cx.simulate_keystrokes(w, "cmd-shift-d");
    cx.run_until_parked();
    let panes = cx.update(|cx| panel.read(cx).tab(0).read(cx).state.layout.panes());
    cx.simulate_keystrokes(w, "cmd-alt-up");
    cx.update(|cx| assert_eq!(panel.read(cx).tab(0).read(cx).state.focused, panes[0]));
    cx.simulate_keystrokes(w, "cmd-alt-down");
    cx.update(|cx| assert_eq!(panel.read(cx).tab(0).read(cx).state.focused, panes[1]));
}

#[gpui_kit::test]
fn a_split_starts_in_the_focused_panes_folder(cx: &mut TestAppContext) {
    // Open a shell tab in a temp dir, split, poll the new pane's cwd() == temp dir.
}

#[gpui_kit::test]
fn a_split_from_a_deleted_folder_starts_in_the_project(cx: &mut TestAppContext) {
    // Shell in temp dir A inside project P; delete A; split; new pane's cwd() == P.
}
```

(Write the last two bodies with the same helpers; the window needs a size large enough to split, e.g. 1200×800.)

- [ ] **Step 2: Run** `cargo test -p sawit terminal::` — Expected: FAIL.
- [ ] **Step 3: Implement** per Interfaces.
- [ ] **Step 4: Run** `cargo test -p sawit terminal::` (the two key-binding tests in `view.rs` must still pass) — Expected: PASS. Manual: split both ways, nest, drag and double-click dividers, check headers and focus border.
- [ ] **Step 5: Commit** `feat(terminal): split panes with headers and draggable dividers`.

---

### Task 6: Pane menu, right-click routing, rename, close pane

**Files:**
- Modify: `crates/app/src/terminal/tab.rs`, `crates/app/src/terminal/view.rs`
- Test: `crates/app/src/terminal/tab.rs`, `crates/app/src/terminal/view.rs`

**Interfaces:**
- Produces:
  - In `view.rs`: `pub enum RightClick { Menu, Program, Nothing }` and `pub fn right_click_target(send_right_clicks: bool, shift: bool, program_has_mouse: bool) -> RightClick` — `shift` → `Menu`; `!send_right_clicks` → `Menu`; else `Program` if `program_has_mouse` else `Nothing`.
  - `TerminalView` emits a new `TerminalEvent::MenuRequested(Point<Pixels>)` when the target is `Menu`; the tab opens the pane menu there (a gpui-component `PopupMenu` anchored at that point; `ContextMenuExt` cannot be made conditional on Shift, so open it programmatically). The view needs the pane's `send_right_clicks`: the tab sets `view.send_right_clicks` whenever `PaneMeta` changes.
  - In `tab.rs`: `pub fn pane_menu_items(zoomed: bool, send_right_clicks: bool) -> Vec<(&'static str, Option<bool>)>` — label plus `Some(checked)` for the toggle:
    `[("Rename pane", None), ("Split right", None), ("Split down", None), (if zoomed {"Unzoom"} else {"Zoom"}, None), ("Send right-clicks to pane", Some(send_right_clicks)), ("Close pane", None)]`.
  - `impl TerminalTab { pub fn close_pane(&mut self, pane: PaneId, window: &mut Window, cx: &mut Context<Self>) }` — removes and drops the view (ends its process); `Removed::Last` emits `TabEvent::Closed`.
  - Rename: "Rename pane" puts an `InputState` in that pane's header (in the tab label when the tab has one pane), prefilled with the current title, selected; Enter → `state.rename`, Escape or blur → cancel.
  - Header and tab title show `PaneMeta.name` over the program's title.

- [ ] **Step 1: Write the failing tests:**

```rust
// view.rs tests
#[test]
fn right_clicks_open_the_menu_unless_sent_to_the_pane() {
    use RightClick::*;
    assert!(matches!(right_click_target(false, false, true), Menu));
    assert!(matches!(right_click_target(true, false, true), Program));
    assert!(matches!(right_click_target(true, false, false), Nothing));
    assert!(matches!(right_click_target(true, true, true), Menu));
}

// tab.rs tests
#[test]
fn pane_menu_matches_herdr() {
    let labels: Vec<_> = pane_menu_items(false, true).into_iter().map(|(l, _)| l).collect();
    assert_eq!(labels, ["Rename pane", "Split right", "Split down", "Zoom", "Send right-clicks to pane", "Close pane"]);
    assert_eq!(pane_menu_items(true, false)[3].0, "Unzoom");
    assert_eq!(pane_menu_items(false, true)[4].1, Some(true));
}

#[gpui_kit::test]
fn closing_panes_closes_the_tab_last(cx: &mut TestAppContext) {
    // open_panel, cmd-d, close_pane(focused) → 1 pane, focus on the first;
    // close_pane(that) → panel.tab_count() == 0.
}
```

- [ ] **Step 2: Run** `cargo test -p sawit terminal::` — Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** — Expected: PASS. Manual: right-click in a shell opens the menu; toggle "Send right-clicks to pane" and right-click in `vim` with `:set mouse=a`; Shift+right-click still opens the menu; rename survives a program setting its title.
- [ ] **Step 5: Commit** `feat(terminal): herdr-style pane menu, rename and close pane`.

---

### Task 7: Zoom key and exited panes

**Files:**
- Modify: `crates/app/src/terminal/tab.rs`, `crates/app/src/terminal/panel.rs`, `crates/app/src/terminal/view.rs`, `crates/app/src/terminal/mod.rs`
- Test: `crates/app/src/terminal/tab.rs`

**Interfaces:**
- Produces:
  - Action `ZoomPane` bound to `cmd-shift-enter` in `"Terminal"` (in `view::bindings()`). Handler on the tab: if `state.toggle_zoom()` returned false, `cx.propagate()` so the workspace's `ToggleMaximizePane` runs.
  - Exited panes: the tab subscribes to each view's `TerminalEvent::Exited`. If `state.is_split()` and the view is a plain shell (`view.shell && view.remote.is_none()`), `close_pane`. Otherwise the pane shows its banner **inside the pane** (move the panel's two banners — "This process has exited." and "Disconnected from {name}." with Reconnect / Close Tab — into the pane's render; in a split, "Close Tab" reads "Close Pane"). `TerminalPanel::reconnect` takes `(tab_ix, pane: PaneId)` and replaces that pane's view.

- [ ] **Step 1: Write the failing tests:**

```rust
#[gpui_kit::test]
fn cmd_shift_enter_zooms_a_split_tab(cx: &mut TestAppContext) {
    // open_panel, cmd-d, cmd-shift-enter → state.zoomed == Some(focused); again → None.
}

#[gpui_kit::test]
fn cmd_shift_enter_with_one_pane_reaches_the_workspace(cx: &mut TestAppContext) {
    // Host wrapping the panel in key_context("Workspace") with
    // .on_action(|_: &crate::workspace::ToggleMaximizePane, ..| count += 1);
    // focus the terminal, cmd-shift-enter → count == 1, tab not zoomed.
}

#[gpui_kit::test]
fn an_exited_shell_in_a_split_closes_its_pane(cx: &mut TestAppContext) {
    // cmd-d; write "exit\n" to the focused view; poll until views().count() == 1.
}

#[gpui_kit::test]
fn an_exited_single_pane_keeps_its_tab(cx: &mut TestAppContext) {
    // write "exit\n"; poll until the view is exited; tab_count() == 1.
}
```

(Make `ToggleMaximizePane` `pub(crate)` if it is not reachable from the test.)

- [ ] **Step 2: Run** `cargo test -p sawit terminal::` — Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `cargo test -p sawit` (whole crate, the workspace binding tests included) — Expected: PASS.
- [ ] **Step 5: Commit** `feat(terminal): zoom panes and close exited shells in splits`.

---

### Task 8: Run, SSH and agent tools see every pane

**Files:**
- Modify: `crates/app/src/terminal/panel.rs`, `crates/app/src/workspace/control.rs`, `crates/mcp/src/app.rs`
- Test: `crates/app/src/terminal/panel.rs` (`#[cfg(test)] mod tests`)

**Interfaces:**
- Produces:
  - Panel lookups search **all panes of all tabs**: `ssh_sessions`, `focus_ssh` (activates tab and focuses that pane), `job_tabs` → `fn job_panes(&self, key: &str, cx: &App) -> Vec<(usize, PaneId)>`, `job_status`, `start_job`, `stop_job`, `show_job`, `run` (live shell = focused pane of the active tab, else newest live shell pane).
  - `set_projects`' "idle shells follow the first project" applies to every pane.
  - `describe` adds `"panes": [{ "index", "title", "focused", "exited", "at_prompt", "running_job", "script", "ssh" }]` per tab (pane index = position in `layout.panes()`); the tab's own fields describe its focused pane.
  - `text`, `input` and `close_tab` gain `pane: Option<usize>`; `None` = focused pane for `text`/`input`, whole tab for `close_tab`. A bad pane index errors with `"Terminal {tab} has no pane {pane}; panes are 0 to {n-1}."`.
  - `crates/mcp/src/app.rs`: `TerminalReadArgs`, `TerminalInputArgs`, `TerminalCloseArgs` gain `#[schemars(description = "Pane within the tab (from app_state); default: the focused pane, or for close, the whole tab")] pub pane: Option<usize>`; `control.rs` passes it through. `app_state`'s description mentions panes.

- [ ] **Step 1: Write the failing tests:**

```rust
#[gpui_kit::test]
fn a_script_in_a_split_is_still_found(cx: &mut TestAppContext) {
    // start_job(tag "k") in tmp dir → its tab; split that tab (cmd-d moves focus to the new pane);
    // job_status("k") is Some; show_job focuses the job's pane, not the new one.
}

#[gpui_kit::test]
fn describe_lists_panes(cx: &mut TestAppContext) {
    // cmd-d; describe()[0]["panes"].as_array().len() == 2, exactly one "focused": true.
}

#[gpui_kit::test]
fn a_bad_pane_index_is_an_error(cx: &mut TestAppContext) {
    // panel.text(Some(0), Some(5), 10, cx) == Err("Terminal 0 has no pane 5; panes are 0 to 0.")
}
```

- [ ] **Step 2: Run** `cargo test -p sawit terminal::` and `cargo test -p sawit-mcp` — Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** both — Expected: PASS.
- [ ] **Step 5: Commit** `feat(terminal): run, ssh and agent tools address panes`.

---

### Task 9: Remove herdr

**Files:**
- Modify: `crates/app/src/terminal/panel.rs` (delete `herdr_available`, `open_herdr`, `herdr_logo`, `install_herdr_button`, the header button, the herdr branch of `render_empty`; empty-state detail becomes "Start a shell in the project folder."; module doc drops herdr), `crates/app/src/terminal/mod.rs` and `view.rs` doc comments ("shell or SSH"; "(vim, Claude Code)"), `crates/app/src/assets.rs` (`HERDR_LOGO` and its match arm), `crates/app/src/tools_page.rs` (`"herdr"` arm), `crates/core/src/dev_tools.rs` (the herdr `Tool`), `crates/app/src/workspace.rs` ("Terminal / herdr" → "Terminal"), `crates/app/src/main.rs` (PATH comment: "docker, brew and friends"), `crates/core/src/terminal/session.rs` (`multiplexer_markers` doc: "a program may refuse to run nested (herdr, tmux, Zellij)"; behaviour unchanged), `scripts/install.sh` (`install_herdr`, the prompt, `SAWIT_HERDR` and its usage line), `README.md` (feature table row, install paragraph, terminal section, design-notes rows: describe split panes and the pane menu instead), `assets/tool_logos/SOURCES.md` (herdr line). Do not edit `CHANGELOG.md`: `scripts/release.sh` writes it from commit messages on push, so this task's commit message carries the entry.
- Delete: `assets/logo/herdr.svg`
- Test: `crates/core/src/dev_tools.rs` tests (if a catalog count/ids test exists, update it)

- [ ] **Step 1: Delete and edit** the items above.
- [ ] **Step 2: Verify nothing else mentions it:** `git grep -ni herdr -- ':!docs/superpowers' ':!CHANGELOG.md'` — Expected: only `crates/core/src/terminal/session.rs` (`HERDR_` marker and its doc) and `crates/core/src/terminal/tests.rs` (marker tests).
- [ ] **Step 3: Run** `cargo test --workspace` and `cargo clippy --workspace -- -D warnings`, and `sh -n scripts/install.sh` — Expected: PASS, no warnings, no syntax errors.
- [ ] **Step 4: Commit** `feat(terminal): split panes replace herdr; Sawit no longer offers or installs it`.
