# Sawit — Design

Date: 2026-10-28. Source: the user's project specification (summarized below)
plus follow-up requests and decisions I made (marked **[my decision]**)
because the user asked: "Just go ahead without this session, you decide."

## Purpose & rationale

A lightweight desktop PostgreSQL GUI for macOS (Apple Silicon) for local development **without Docker**.
User: a single developer with several projects, each with its own local DB, who once
silently connected to the wrong DB because Homebrew bound `127.0.0.1:5433` while Docker bound `*:5433`.

The user's follow-up requests (gist, verbatim):
- "A PostgreSQL GUI that can manage PostgreSQL tables, runs without Docker, has MCP"
- "so my TanStack projects won't need Docker or anything else installed going forward"
- "Make sure this is the lightest compared to the others."

## Scope expansion (2026-10-29)

User: "I want sawit to have a text editor too", "is that possible the terminal are running
https://herdr.dev/? ... i need text editor + GUI Postgresql. So far I've been using zed + herdr + docker.
That's very heavy, I just want all in one. For design, please use zed design." and "Don't stop,
keep going until everything is done".

So Sawit = **one all-in-one window** replacing Zed + herdr + Docker for the user's workflow:
- **Text editor** (stage E): project file tree, file tabs, `gpui-component` code editor (`InputState::code_editor`,
  tree-sitter for sql/ts/tsx/js/json/md/toml/rust/css/html), save (Cmd-S), unsaved indicator.
  Not an IDE: no LSP, no debugger, no git UI in this scope **[my decision]**.
- **Terminal** (stage T): a real terminal emulator in the bottom panel (PTY + `alacritty_terminal`, like Zed),
  complete enough to run the **herdr** TUI (alt-screen, 256/truecolor, mouse, resize, bracketed paste).
  herdr runs as a normal program in the terminal (`herdr` on PATH); Sawit does not bundle herdr.
- **Zed-style design**: Zed layout (compact title bar, left panel, center tab area, bottom dock, status bar),
  palette from Zed's official "One Dark" theme (taken from `zed-industries/zed` `assets/themes/one/one.json`),
  small, dense UI text like Zed.

New work order **[my decision]**: Stage 1 (server) → Zed-style shell + Terminal (highest technical risk,
proven early) → Text editor → Stage 2 (connections & schema) → Stage 3 (grid) → Stage 4 (SQL editor, using
the same editor) → Stage 5 (MCP + `.env`).

## Goals

1. **Server manager**: list instances (name, version, port, data dir, status); start/stop/restart via
   `pg_ctl` with a live log; detect port conflicts before starting (other processes, Homebrew service, Docker
   container), including the `127.0.0.1` vs `*` case on the same port; installed binaries (Homebrew/Postgres.app,
   detected, not hardcoded) **and** binaries the app downloads itself.
2. **Connections & schema explorer**: profiles (host, port, db, user; password in the macOS Keychain); import
   `DATABASE_URL` from a `.env` the user picks; tree database → schema → table/view + columns & types.
3. **Read-only data grid**: virtual table, server-side paging, smooth for hundreds of thousands of rows.
4. **SQL editor**: multi-line, run, show results/errors/duration. Highlighting = stretch.
5. **MCP & project integration** **[my decision: stage 5, after the core MVP]**: an MCP server (stdio) so
   AI agents can see instances and schemas and run queries; write `DATABASE_URL` into a TanStack project's `.env`
   from a managed instance.

"Manage tables" **[my decision]**: in the MVP this is covered by the schema explorer + grid + SQL editor (DDL/DML
via SQL). Inline data editing stays out of MVP scope per the original spec; it could be a later stage.

## Non-goals

Inline data editing (MVP), visual schema design, databases other than Postgres, cloud sync.

## Anti-goals (failures even if it "works")

- UI thread blocked by DB or process I/O.
- A port conflict slips through without a warning → the user connects to the wrong DB.
- Passwords stored in plaintext.
- A heavy app (Electron/JVM-class). See the lightweight target below.
- TanStack projects still need Docker/Homebrew to get Postgres.

## Technical constraints

- Rust stable (machine: 1.97.1), a single Cargo workspace; `core` crate with no GPUI dependency, `app` crate for UI only.
- Target macOS Apple Silicon; Homebrew path detected (`brew --prefix`), not hardcoded.
- Don't write GPUI APIs from memory: pin the exact version and check against that version's source.
- Every stage: `cargo build`, `cargo test`, `cargo clippy --workspace --all-targets -- -D warnings` clean, then commit.

## Architecture decisions

| Area | Decision | Rationale |
|---|---|---|
| UI | `gpui-kit = "=0.8.1"` (official Longbridge facade: GPUI + gpui-component + assets) | Finding: `gpui-component` 0.8.1 does **not** use `gpui` 0.2.2 from crates.io, but the snapshot `gpui-pre =0.3.10` + `gpui-pre-platform` with the `runtime_shaders` feature (no full Xcode needed; this machine only has the Command Line Tools). `gpui-kit` pins the matching set. Reference source: tag `v0.8.1`, commit `99576db58ec8103417c41a092e891b4022d24e7e`. |
| DB driver | `tokio-postgres` 0.7.x, `NoTls` for local | Lighter than `sqlx` (no compile-time macros/built-in pool); `simple_query` suits a free-form SQL editor; cursor/portal for paging. |
| Async | Multi-thread Tokio runtime on its own thread (`core::rt`), results sent via `futures::channel::oneshot`/`mpsc` and awaited in GPUI's `cx.spawn` | `futures` channels aren't tied to an executor; the UI never blocks. |
| Postgres binaries | Detect Homebrew (`brew --prefix` → `opt/postgresql@*`), Postgres.app (`/Applications/Postgres.app/Contents/Versions/*`), `PATH`; plus self-download via **`postgresql_archive`** (features `theseus`, `tls-rustls-ring`) | Evaluated `postgresql_embedded` 0.22: it pulls in `sqlx` and manages its own lifecycle (clashing with our `pg_ctl` control). Its underlying crate `postgresql_archive` is enough to download+extract binaries → lighter. This is also what frees TanStack projects from Docker/Homebrew. |
| Process control | `initdb`, `pg_ctl start -l <log> -o "-p <port> -c listen_addresses=<...> -k <sockdir>"`, `pg_ctl stop -m fast`, `pg_ctl status`; live log = tail the log file | `pg_ctl start` detaches the process; tailing the log file also works for instances started outside the app. |
| Port detection | `lsof -nP -iTCP:<port> -sTCP:LISTEN -F pcn` (field format, stable to parse) + `ps -o command= -p <pid>` + `docker ps --format` when `docker` exists | No root needed for the user's own processes. Verified on this machine: Docker shows up as `com.docker.backend` with `n*:5433`. |
| Storage | TOML in `~/Library/Application Support/sawit/` (`instances.toml`, `profiles.toml`); passwords in the Keychain via the `keyring` crate | Human-readable, easy to diff; secrets never go to files. |
| Managed instances | Data dir at `~/Library/Application Support/sawit/clusters/<name>`; downloaded binaries at `.../sawit/bin/<version>`; `listen_addresses=localhost`, socket dir under the app dir | Isolated from Homebrew; no clash with the `/tmp` socket. |
| MCP | `rmcp` crate (stdio), separate `sawit-mcp` binary using `core`; queries read-only by default (`SET TRANSACTION READ ONLY`) | Agents can't damage data without explicit permission. |

### Port conflict semantics

Bind addresses are modeled: IPv4 wildcard (`*`/`0.0.0.0`), IPv6 wildcard (`[::]`), IPv4/IPv6 loopback, specific address.
For the target port and the addresses our instance will use:
- **Free**: no listener.
- **Conflict**: a listener on exactly the same address → start will fail; explain who owns it.
- **Shadowing**: another listener on an address of different specificity (e.g. us `127.0.0.1`, Docker `*`)
  → start may *succeed*, but clients using `localhost` and clients using another IP reach different servers.
  This is the case that bit the user; the warning must name both sides and which one wins for `127.0.0.1`.
Owners are identified as: Docker (+ container name from `docker ps`), Homebrew (binary path under `brew --prefix`),
Postgres.app, a Sawit instance (matching data dir), or another process (name + PID).

### "Lightest" target [my decision]

Measured and recorded in the README: release binary size (`lto = "thin"`, `strip = true`), idle RSS with one window.
Target: idle RSS < 150 MB, no JVM/Chromium runtime. No comparison figures for other apps are claimed without measuring on the same machine.

## Work order

0. Integration spike (GPUI window + DataTable + `SELECT version()` via Tokio, UI doesn't freeze) → record API findings.
1. Stage 1 (detailed plan: `docs/superpowers/plans/2026-10-28-sawit-stage1-server-manager.md`).
2–5. Each stage's plan is written after the previous stage finishes, using real API findings from the previous stage.

## Left to the builder

Detailed visual layout, UI text, internal module structure beyond the interfaces set by the plan.
