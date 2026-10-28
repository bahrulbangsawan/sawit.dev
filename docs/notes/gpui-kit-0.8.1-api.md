# Catatan API nyata: gpui-kit 0.8.1 (gpui-pre 0.3.10)

Sumber yang dicocokkan: `longbridge/gpui-component` tag `v0.8.1`
(commit `99576db58ec8103417c41a092e891b4022d24e7e`) dan `gpui-pre-0.3.10` di registry Cargo.

## Temuan yang berbeda dari dugaan awal

1. **`gpui` di crates.io (0.2.2, Okt 2025) tidak dipakai.** gpui-component 0.8.1 mengunci
   snapshot `gpui-pre =0.3.10` + `gpui-pre-platform` (fitur `runtime_shaders`). Fasad
   `gpui-kit =0.8.1` mengunci set yang cocok → aplikasi cukup bergantung pada `gpui-kit`.
   `use gpui_kit::*` = GPUI; komponen di `gpui_kit::component::*`.
2. **Butuh Rust ≥ 1.99.** `gpui-pre-macos 0.3.10` memakai `Option::map_or_default`
   (unstable di 1.97.1). Repo mengunci `rust-toolchain.toml` → `1.99.1` (stable terbaru, 2026-10-13).
3. **Xcode penuh tidak diperlukan.** `runtime_shaders` mengompilasi shader Metal saat runtime;
   build berhasil hanya dengan Command Line Tools.
4. **Jendela tertutup tidak di-render.** `gpui-pre-macos` menghentikan display link saat
   `occlusionState` tidak terlihat (hemat daya). Ini bukan freeze; state tetap ter-update.
   Aplikasi memanggil `cx.activate(true)` setelah membuka jendela.

## Pola yang terbukti (spike)

- Bootstrap: `gpui_kit::application().run(|cx| { gpui_kit::init(cx); gpui_kit::open_window(opts, cx, |window, cx| cx.new(|cx| View::new(window, cx))) })`.
- Global: `struct Rt(Runtime); impl Global for Rt {}` → `cx.set_global(Rt(rt))`, `cx.global::<Rt>()`.
- Async: `let rx = cx.global::<Rt>().0.spawn(fut);` lalu
  `cx.spawn(async move |this: WeakEntity<T>, cx: &mut AsyncApp| { let v = rx.await; this.update(cx, |this, cx| ...) }).detach()`.
  Signature: `Context::spawn<AsyncFn: AsyncFnOnce(WeakEntity<T>, &mut AsyncApp) -> R>`.
- Timer di executor GPUI: `cx.background_executor().timer(Duration)`.
- Tabel: `TableDelegate` (`columns_count`, `rows_count`, `column`, `render_td(row, col, &mut Window, &mut Context<TableState<Self>>) -> impl IntoElement`);
  `cx.new(|cx| TableState::new(delegate, window, cx))`; render dengan `DataTable::new(&entity).bordered(true)`;
  ubah data via `entity.update(cx, |t, cx| { t.delegate_mut().rows = ..; t.refresh(cx) })`.
  Untuk Tahap 3 tersedia `visible_rows_changed`, `has_more`, `load_more_threshold`, `load_more`.
- Tombol: `Button::new(id).primary().label(..).on_click(cx.listener(|this, _ev, _window, cx| ..))`.
- Layout: `v_flex()`, `h_flex()`, `div().flex_1()`; `window.set_window_title(..)`; `window.is_window_active()`.

## Angka spike (debug, M-series)

- Build bersih pertama: ±5,5 menit (unduh + kompilasi seluruh pohon gpui); rebuild app: ±97 detik.
- `SELECT pg_sleep(3), version()` via Tokio: 29 tick UI (100 ms) dan 33 frame dirender selama 3,0 detik → UI tidak terblokir.
- RSS debug idle ±86–90 MB.

## Pelajaran lain

- macOS: path socket Unix maks 103 byte. Path scratchpad yang panjang membuat postmaster gagal
  start (`Unix-domain socket path ... is too long`). Ini membenarkan `validate_socket_dir` (Task 9).

## postgresql_archive 0.22.0

- `install_managed("=16", root)` unduh PostgreSQL 16.15.1 (theseus, aarch64-apple-darwin) dalam ±43 detik.
- Extractor theseus mengekstrak ke tempdir lalu `rename` atomik ke dir tujuan; **dilewati bila dir tujuan sudah ada**.
