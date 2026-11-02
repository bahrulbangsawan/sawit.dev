---
name: sawit-terminal
description: Run dev servers, scripts, tests and long commands in Sawit's terminals and Run panel, and handle ports and SSH, instead of hidden background shells, nohup or kill -9. Use it when you start or restart a dev server, run a package.json or Makefile script, watch a test runner, find or free a busy port, or run a command on a server over SSH, through the sawit MCP server (terminal_run, app_run_script, list_ports, stop_processes, ssh_exec).
---

# Terminals, scripts, ports and SSH in Sawit

What you run in Sawit's terminal shows in the user's window, where they can watch and stop it. Use it for anything that keeps running: dev servers, watchers, migrations, builds. A quick command whose output you only need can stay in your own shell. The terminal tools need the app (`launch_app`).

## Scripts and dev servers

1. `app_run_list` (or `list_scripts` with `project_dir`, which works without the app) lists the project's scripts (package.json with workspaces, Makefile, cargo, deno, composer) and saved commands, with their exact names.
2. `app_run_script` with that `name` starts it in a terminal tab of its own and returns the tab and `pane`: keep both.
3. `terminal_read` with that tab and pane reads its output: look for the ready line or the errors. `app_stop_script` stops it.
4. `save_command` saves command lines to the project's Run panel (`mode`: sequence or parallel) when the project has no script for them.

## Any command

- `terminal_run` types a command and returns where it ran ("Terminal 2, pane 0") with its latest output (`wait_secs` up to 120, `new_tab`, `cwd`).
- `terminal_split` runs something beside a terminal: a server next to a test watcher.
- Pass the tab and pane you got back to `terminal_read`, `terminal_input` and `terminal_close`: the terminal in front may be one the user types in. `terminal_input` with `key` ctrl-c interrupts; `text` answers a prompt.
- `terminal_close` ends what runs there; a running program, or the user typing there, needs `confirm`: ask first.

## Ports

- `list_ports` lists the dev servers that listen (PID, port, command, folder); `all` adds databases, Docker and daemons. `check_port` says what holds one port.
- `stop_processes` stops them by `ports` or `pids` and needs `confirm`; it skips Docker, editors, agents and Sawit instances. Stop only what you started or the user named.

## SSH

- `list_ssh_hosts` lists saved servers and `~/.ssh/config` hosts; `probe_ssh_host` checks that one answers.
- `ssh_exec` runs one non-interactive command and returns its exit code and output.
- `app_ssh_connect` opens an interactive session in Sawit's terminal; drive it with `terminal_input` and `terminal_read`.
- `add_ssh_host` saves a server; a password goes to the Keychain, never a file.

## Without the sawit MCP server

Tell the user, as the `sawit` skill says. In a shell, `sawit list ports` shows the dev servers and `sawit bakar ports 3000` stops one (it asks first). Run long commands where the user can see them, and stop what you start.
