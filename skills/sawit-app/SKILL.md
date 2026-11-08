---
name: sawit-app
description: Drive the Sawit app itself: start it, read what it shows (windows, tabs, terminals, browser, sign-ins, agents at work), open files and folders, panels, pages and tabs, change the layout, read and change key bindings, search a folder's text, and manage Claude Code accounts and what agents may do (permissions). Use it to show the user something in Sawit or change how Sawit is set up (app_state, app_open, app_open_page, keymap_bind, list_agent_accounts, edit_permission).
---

# The Sawit app

Most Sawit tools work without the app; the `app_*` and `keymap_*` tools need it. `launch_app` starts it (or brings it forward), with `path` to open a folder or file. Never take the keyboard: these tools show things beside the user's work, and the user stays in charge of focus.

## See what the user sees

`app_state` is the first call: each window's project, its sidebar panel and layout, the open tabs (index, title, kind, path, unsaved), the terminal tabs and panes (what runs there, which coding agent), the browser tab, `github` (signed in and login; only the user signs in) and `agents` (the AI agents at work on a canvas or browser now, you among them).

## Show things

- `app_open` opens a folder as a project, or a file at a `line` and `column` (`new_window` for a window of its own).
- `app_show_panel` with `panel`: instances, files, git, database, run, ports, network, prompts, ssh or shell_config.
- `app_open_page` with `page`: canvas, browser, mcp, tools, skills, changelog, git_graph, github, instance, keymap, new_project, permissions or accounts.
- `app_open_table`, `app_open_sql` (`run` runs it in the app), `app_open_diagram`, `app_open_prompt` and `app_instance` open data, prompts and instances in their tabs (see `sawit-databases` and `sawit-prompts`).
- `app_layout` shows or hides the sidebar, the tabs and the terminal dock; `app_maximize` with `panel` (terminal, browser or canvas) fills the window and back; `app_focus` brings a window forward.
- `app_pomodoro` drives the user's Pomodoro timer (25 minutes of focus, then a break): `state`, `start`, `pause`, `skip`, `stop`, `show` or `hide`. It is the user's own timer: change it only when they ask; `app_state`'s `pomodoro` says how it stands.
- `app_activate_tab` and `app_close_tab` by index or title. A tab that would lose something asks the user and answers "asking". `app_close_all_tabs` is Safe Close All; `force` closes everything and needs `confirm`: ask first. `app_tab` does a tab's menu action (its `action`): close the others, the ones to the left or right, or every clean one; pin or unpin; make a file's tab read-only or editable.
- Most take `project`: the folder of the window to act in, when several are open.

## Search

`search_files` with `dir` and `query` finds text in a folder's files, skipping what .gitignore ignores; it works without the app.

## Key bindings

- `keymap_list` (`filter`, `keys`, `unbound`) shows each binding, its keys, context, where it comes from and clashes.
- `keymap_bind` (`keys`, `action`, `context`, `replace`) and `keymap_unbind` change the user's bindings at once, with the Keymap Editor's checks, and say what a new binding clashes with. Change keys only when the user asks; `restore: true` brings back a default. A keymap.json with comments is refused unless `confirm`, since it is written again without them.

## Agent accounts

- `list_agent_accounts`: each account, the active one, and the 5-hour and 7-day usage Claude Code last reported; `agent_account_usage` (`check: true` asks for current numbers, which spends a few hundred tokens of that account).
- `agent_account_sessions`: the running Claude Code sessions per account.
- `account_pool` (`view`: overview, detailed, compare, timeline, …; `provider`; `days`): the pool of Claude Code and Codex accounts as the Accounts page shows it. Before long work, `view: "compare"` names the account with the most room left and why.
- `use_agent_account` makes one active for new sessions; `open_agent_account` opens a terminal running `claude` as an account (`folder`). `manage_agent_account` renames one (with `name`), moves it up or down the list, or takes it off Sawit's list (with `confirm: true`; its folder stays).
- `sync_agent_settings` with `action` status or run: the MCP servers, skills, plugins, hooks and rules shared across accounts, when the user turned sharing on. Signing in is the user's.

## Permissions

- `list_permissions`: Claude Code's allow, ask and deny rules in each settings file, which Sawit tools need `confirm`, the other agents' permission settings and the macOS permissions Sawit has.
- `edit_permission` adds, removes or moves one rule (`file`: user, user:<profile>, project or local; `action`; `rule`; `to`). Without `confirm` it returns the diff; ask the user before you pass `confirm: true`, and never widen what you may do on your own.

## Without the sawit MCP server

Tell the user, as the `sawit` skill says. In a shell: `sawit <path>` opens a folder or file, `sawit list keys` shows the key bindings, `sawit account list|use|open|usage`, and `sawit permissions` (with `add`, `move`, `remove`, which ask first).
