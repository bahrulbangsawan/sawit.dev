# PG Desk — Terminal, Editor, Database (Stages T, E, 2–5) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: superpowers:executing-plans. Steps use checkbox (`- [ ]`) syntax.

**Goal:** Complete the all-in-one PG Desk: a terminal that runs herdr, a project text editor, connections + schema explorer, a virtual data grid, an SQL editor, MCP + `.env` integration.

**Architecture:** Non-UI logic lives in `pgdesk-core` (terminal PTY/emulation, file tree, connection/schema/query, keychain, dotenv, MCP tools); the `pgdesk` app only draws and forwards input. A separate `pgdesk-mcp` binary uses core.

**Spec:** `docs/superpowers/specs/2026-10-28-pgdesk-design.md` (section "Scope expansion").

## Global Constraints
Same as the Stage 1 plan (core without GPUI, pinned UI version, UI never blocks, build/test/clippy gate + commit per task). Rust 1.99.1 (rust-toolchain.toml).

## Review Focus
1. **herdr in the terminal**: alt-screen, SGR mouse, resize, truecolor, Ctrl combinations — must be fully usable. Verify manually with herdr 0.8.2 installed.
2. **Large / binary files in the editor**: opening a file >5 MB or non-UTF-8 must not freeze or corrupt anything; show a read-only message.
3. **Tables with hundreds of thousands of rows**: the grid only fetches the visible pages (keyset/OFFSET per 200 rows), scrolling stays smooth.
4. **Passwords**: never written to a file; Keychain only.
5. **MCP**: read-only by default (`SET TRANSACTION READ ONLY`); write statements are rejected unless an explicit flag is set.

### Task T1: `core::terminal` — PTY + emulation + input encoding
Interfaces: `Session::spawn(Command, GridSize) -> Result<(Session, UnboundedReceiver<TermEvent>)>`, `write`, `resize`, `scroll`, `snapshot(&Palette) -> Snapshot`; `key_to_bytes(&KeyInput, Modes)`, `mouse_report(MouseAction, col, row, shift, Modes)`, `paste_bytes`.
Tests: see `crates/core/src/terminal/tests.rs` (12 tests, including a real PTY).

### Task T2: Terminal panel in the bottom dock (app)
`TerminalView` (canvas: measure cells from the mono font, resize the session when bounds change, draw per-run background + per-run text + cursor), focus & keyboard → `key_to_bytes`, Cmd-V paste, Cmd-C copy selection (optional), mouse → `mouse_report` or history scroll, multiple terminal tabs (+), a "herdr" button that opens a tab running `herdr` if it is on PATH. Manual verification: run herdr, navigate with keyboard & mouse, resize the panel.

### Task E1: `core::files` — project file tree
`list_dir(path) -> Vec<Entry{name, path, is_dir}>` (dirs first, hide `.git`, `node_modules`, `target`), `read_text(path) -> Result<TextFile{text, read_only_reason}>` (5 MB limit, detect non-UTF-8), `write_text_atomic(path, text)`. TDD with tempdir.

### Task E2: Text editor (app)
Tabbed left panel: Servers | Files | Database. Files: open a project folder (prompt), lazy tree. Tabbed center area (Zed style): file tabs with `InputState::code_editor(lang)`, unsaved indicator, Cmd-S save, Cmd-W close. Language from extension (sql, ts, tsx, js, jsx, json, md, toml, rs, css, html, yaml).

### Task D1: `core::db` — profiles, dotenv, keychain, schema
`ConnProfile{name, host, port, database, user}` (TOML), `parse_database_url(&str) -> Result<(ConnProfile, Option<String> password)>`, `read_dotenv_database_url(path)`, password via `keyring` (service `pgdesk`, account `user@host:port/db`), `load_schema(client) -> Vec<SchemaNode>` (schema → table/view → columns+types from `information_schema`/`pg_catalog`).

### Task D2: Database panel + schema tree (app)
Add connection (form / import `.env` / from a PG Desk instance), tree database→schema→table/view→columns.

### Task D3: Virtual read-only data grid
`core::db::fetch_page(client, table, offset, limit)` + `count_estimate`; the grid tab uses `TableDelegate` (`rows_count` = estimate/total, `visible_rows_changed` triggers fetching a 200-row page, LRU page cache).

### Task D4: SQL editor
"SQL" tab per connection: code_editor with sql language, Cmd-Enter runs (`simple_query`), results in the grid, errors & duration; selected statement or the whole contents.

### Task M1: MCP + project integration
`pgdesk-mcp` binary (rmcp stdio): tools `list_instances`, `start_instance`, `stop_instance`, `list_schemas`, `describe_table`, `query` (read-only by default). A "Write DATABASE_URL to .env" button on the instance (pick a project folder, update/append the line).

### Task Z: README + size/RSS + final review
