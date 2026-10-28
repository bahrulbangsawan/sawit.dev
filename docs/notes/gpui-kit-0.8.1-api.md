# Real API notes: gpui-kit 0.8.1 (gpui-pre 0.3.10)

Sources checked against: `longbridge/gpui-component` tag `v0.8.1`
(commit `99576db58ec8103417c41a092e891b4022d24e7e`) and `gpui-pre-0.3.10` in the Cargo registry.

## Findings that differ from initial assumptions

1. **`gpui` on crates.io (0.2.2, Oct 2025) is not used.** gpui-component 0.8.1 pins
   the snapshot `gpui-pre =0.3.10` + `gpui-pre-platform` (feature `runtime_shaders`). The
   `gpui-kit =0.8.1` facade pins a matching set → the app only needs to depend on `gpui-kit`.
   `use gpui_kit::*` = GPUI; components live in `gpui_kit::component::*`.
2. **Requires Rust ≥ 1.99.** `gpui-pre-macos 0.3.10` uses `Option::map_or_default`
   (unstable in 1.97.1). The repo pins `rust-toolchain.toml` → `1.99.1` (latest stable, 2026-10-13).
3. **Full Xcode is not needed.** `runtime_shaders` compiles Metal shaders at runtime;
   the build succeeds with just the Command Line Tools.
4. **Occluded windows are not rendered.** `gpui-pre-macos` stops the display link when
   `occlusionState` is not visible (to save power). This is not a freeze; state keeps updating.
   The app calls `cx.activate(true)` after opening the window.

## Proven patterns (spike)

- Bootstrap: `gpui_kit::application().run(|cx| { gpui_kit::init(cx); gpui_kit::open_window(opts, cx, |window, cx| cx.new(|cx| View::new(window, cx))) })`.
- Global: `struct Rt(Runtime); impl Global for Rt {}` → `cx.set_global(Rt(rt))`, `cx.global::<Rt>()`.
- Async: `let rx = cx.global::<Rt>().0.spawn(fut);` then
  `cx.spawn(async move |this: WeakEntity<T>, cx: &mut AsyncApp| { let v = rx.await; this.update(cx, |this, cx| ...) }).detach()`.
  Signature: `Context::spawn<AsyncFn: AsyncFnOnce(WeakEntity<T>, &mut AsyncApp) -> R>`.
- Timer on the GPUI executor: `cx.background_executor().timer(Duration)`.
- Table: `TableDelegate` (`columns_count`, `rows_count`, `column`, `render_td(row, col, &mut Window, &mut Context<TableState<Self>>) -> impl IntoElement`);
  `cx.new(|cx| TableState::new(delegate, window, cx))`; render with `DataTable::new(&entity).bordered(true)`;
  change data via `entity.update(cx, |t, cx| { t.delegate_mut().rows = ..; t.refresh(cx) })`.
  For Stage 3 there are `visible_rows_changed`, `has_more`, `load_more_threshold`, `load_more`.
- Button: `Button::new(id).primary().label(..).on_click(cx.listener(|this, _ev, _window, cx| ..))`.
- Layout: `v_flex()`, `h_flex()`, `div().flex_1()`; `window.set_window_title(..)`; `window.is_window_active()`.

## Spike numbers (debug, M-series)

- First clean build: ~5.5 minutes (download + compile the whole gpui tree); app rebuild: ~97 seconds.
- `SELECT pg_sleep(3), version()` via Tokio: 29 UI ticks (100 ms) and 33 frames rendered over 3.0 seconds → UI not blocked.
- Idle debug RSS ~86–90 MB.

## Other lessons

- macOS: Unix socket paths are limited to 103 bytes. A long scratchpad path made the postmaster fail
  to start (`Unix-domain socket path ... is too long`). This justifies `validate_socket_dir` (Task 9).

## postgresql_archive 0.22.0

- `install_managed("=16", root)` downloads PostgreSQL 16.15.1 (theseus, aarch64-apple-darwin) in ~43 seconds.
- The theseus extractor extracts to a tempdir, then does an atomic `rename` to the target dir; **skipped if the target dir already exists**.
