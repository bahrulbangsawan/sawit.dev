---
name: sawit-terminal
description: Run dev servers, scripts, tests and long commands in Sawit's terminals and Run panel, and handle busy ports, the network and Cloudflare Workers dev servers, instead of hidden background shells, nohup or kill -9. Use it when you start or restart a dev server, run a package.json or Makefile script, watch a test runner, find or free a busy port, check the connection, or read a Workers dev server's KV, D1 or R2 (terminal_run, terminal_read, app_run_script, list_ports, stop_processes, local_explorer).
---

# Terminals, scripts and ports in Sawit

What you run in Sawit's terminal shows in the user's window, where they can watch and stop it. Use it for anything that keeps running: dev servers, watchers, migrations, builds. A quick command whose output you only need can stay in your own shell. The terminal tools need the app (`launch_app`).

## Scripts and dev servers

1. `app_run_list` (or `list_scripts` with `project_dir`, which works without the app) lists the project's scripts (package.json with workspaces, Makefile, cargo, deno, composer) and saved commands, with their exact names.
2. `app_run_script` with that `name` starts it in a terminal tab of its own and returns the tab and `pane`: keep both.
3. `terminal_read` with that tab and pane reads its output: look for the ready line or the errors. `app_stop_script` stops it.
4. `save_command` saves command lines to the project's Run panel (`mode`: sequence or parallel) when the project has no script for them; `remove_command` takes one out (`confirm`).

## Any command

- `terminal_run` types a command and returns where it ran ("Terminal 2, pane 0 (run 5)") with its latest output (`wait_secs` up to 120, `new_tab`, `cwd`). Your next `terminal_run` goes to your own latest terminal that is idle (with a `cd` when `cwd` differs), so many commands leave one tab: don't pass `new_tab` unless you need a second shell. A locked or pinned tab, one running something and the one the user types in are never used.
- `close_when_done: true` closes a tab opened for a one-off command once it finishes; `terminal_read` with `run` (the number from the answer) still reads its output afterwards. Read by `run` whenever you have it: tab numbers change when other tabs close.
- `terminal_split` runs something beside a terminal: a server next to a test watcher.
- `terminal_rename` labels the tab (or a pane of a split tab) where it runs, so the user can tell them apart.
- Pass the tab and pane you got back to `terminal_read`, `terminal_input` and `terminal_close`: the terminal in front may be one the user types in. `terminal_input` with `key` ctrl-c interrupts; `text` answers a prompt.
- `terminal_close` ends what runs there; a running program, or the user typing there, needs `confirm`: ask first. With `scope` (others, left, right, idle or all) it closes other tabs as the tab menu does; `idle` is Safe Close All.
- `terminal_lock` keeps a tab from being closed until unlocked, `terminal_pin` pins it to the strip's start, and `terminal_clear` clears its scrollback: for the tabs you run things in, when it helps the user.

## Ports

- `list_ports` lists the dev servers that listen (PID, port, command, folder); `all` adds databases, Docker and daemons. `check_port` says what holds one port.
- `stop_processes` stops them by `ports` or `pids` and needs `confirm`; it skips Docker, editors, agents and Sawit instances. Stop only what you started or the user named.

## Network

`diagnose_network` checks the connection, the router, DNS, the internet and a Wi-Fi sign-in page, and `ping` a host; `app_network_diagnose` runs the same on the app's Network tab for the user to see. Run them before you blame a failing download or API call on the code.

## Cloudflare Workers dev servers

`local_explorer` lists the Local Explorers that running `wrangler dev` or Vite plugin servers serve (URL, port, Workers and their bindings) and the project's Workers. `local_explorer_request` calls one's API (`path`: local/workers, storage/kv/namespaces, d1/database, r2/buckets, …): GET reads; POST, PUT, PATCH and DELETE change the dev server's data and need `confirm`. For D1 SQL, `query` and `execute_sql` with a `d1-local:` target are simpler (see `sawit-databases`).

SSH is in `sawit-ssh`.

## Without the sawit MCP server

Tell the user, as the `sawit` skill says. In a shell, `sawit list ports` shows the dev servers, `sawit bakar ports 3000` stops one (it asks first) and `sawit list explorer` the Workers dev servers. Run long commands where the user can see them, and stop what you start.
