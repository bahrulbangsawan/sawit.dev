# Sawit Skills v2: every tool and command, and the skill that teaches it (B11)

The Sawit pack after B11: ten skills, one router. Each tool family is taught by one skill; a tool
named in two (a terminal tool in `sawit-ssh`, say) is taught where its family lives and pointed
to from the other. `crates/mcp/src/tests/sawit_skills.rs` fails when a tool the server lists is
named by no skill (outside the "not taught" list below), when the router doesn't route to a
skill, or when a description is outside 80–600 characters. Built on B1's audit
(`2026-11-07-mcp-audit.md`) and the tools in `backlog` at commit time (172).

## The skills

| Skill | Families |
|---|---|
| `sawit` | The router: which skill for which job, ground rules, code mode (`search_api`, `run`), the CLI fallback |
| `sawit-databases` | Instances, databases, SQL, connections, `.env`, D1 and SQLite, exports, backups, imports, destructive tools |
| `sawit-terminal` | Terminals (reuse, run numbers, lock, pin, close), the Run panel, ports, the network, Cloudflare Local Explorer |
| `sawit-ssh` | Hosts, probing, `ssh_exec`, interactive sessions |
| `sawit-git` | Git status to push; the Git tab, commit graph and GitHub page |
| `sawit-browser-testing` | The browser loop, tabs, SEO and agent readiness, design, Annotate (mesurer), annotations |
| `sawit-canvas` | Screens, vector work, starter system and lint, diagrams, canvases, routes, settings, handoff |
| `sawit-project` | New Project, dev tools, MCP servers, skill packs, Sawit Skills, config files, shell files sync |
| `sawit-prompts` | The prompt library and its GitHub backup and sync |
| `sawit-app` | App state, windows, panels, pages, tabs, layout, file search, key bindings, agent accounts and pool, permissions |

`sawit-canvas-design` stays its own pack (the design standard, also served by `canvas_design_guide`).

## Tools

| Tool | Taught by | Note |
|---|---|---|
| `account_pool` | sawit-app |  |
| `add_ssh_host` | sawit-ssh |  |
| `agent_account_sessions` | sawit-app |  |
| `agent_account_usage` | sawit-app |  |
| `app_activate_tab` | sawit-app |  |
| `app_close_all_tabs` | sawit-app |  |
| `app_close_tab` | sawit-app |  |
| `app_focus` | sawit-app |  |
| `app_instance` | sawit-databases, sawit-app |  |
| `app_layout` | sawit-app |  |
| `app_maximize` | sawit-app |  |
| `app_maximize_terminal` | — | not taught: deprecated alias |
| `app_network_diagnose` | sawit-terminal |  |
| `app_open` | sawit-app |  |
| `app_open_diagram` | sawit-databases, sawit-app |  |
| `app_open_page` | sawit-git, sawit-canvas, sawit-app |  |
| `app_open_prompt` | sawit-prompts, sawit-app |  |
| `app_open_sql` | sawit-databases, sawit-app |  |
| `app_open_table` | sawit-databases, sawit-app |  |
| `app_run_list` | sawit-terminal |  |
| `app_run_script` | sawit-terminal |  |
| `app_show_panel` | sawit-git, sawit-app |  |
| `app_ssh_connect` | sawit-ssh |  |
| `app_state` | sawit-git, sawit-app |  |
| `app_stop_script` | sawit-terminal |  |
| `backup_database` | sawit-databases |  |
| `backup_prompts` | sawit-prompts |  |
| `browser_annotations` | sawit-browser-testing |  |
| `browser_click` | sawit-browser-testing |  |
| `browser_console` | sawit-browser-testing |  |
| `browser_design` | sawit-browser-testing |  |
| `browser_evaluate` | sawit-browser-testing |  |
| `browser_export_design` | sawit-browser-testing |  |
| `browser_hover` | sawit-browser-testing |  |
| `browser_inspect` | sawit-browser-testing |  |
| `browser_mesurer` | sawit-browser-testing |  |
| `browser_navigate` | sawit-browser-testing |  |
| `browser_network` | sawit-browser-testing |  |
| `browser_press_key` | sawit-browser-testing |  |
| `browser_resize` | sawit-browser-testing |  |
| `browser_screenshot` | sawit-browser-testing |  |
| `browser_scroll` | sawit-browser-testing |  |
| `browser_select_option` | sawit-browser-testing |  |
| `browser_seo` | sawit-browser-testing |  |
| `browser_snapshot` | sawit-browser-testing |  |
| `browser_tabs` | sawit-browser-testing |  |
| `browser_type` | sawit-browser-testing |  |
| `browser_wait_for` | sawit-browser-testing |  |
| `canvas_add_diagram` | sawit-canvas |  |
| `canvas_add_elements` | sawit-canvas |  |
| `canvas_add_mermaid` | sawit-canvas |  |
| `canvas_add_routes` | sawit-canvas |  |
| `canvas_begin` | sawit-canvas |  |
| `canvas_clear` | sawit-canvas |  |
| `canvas_command` | sawit-canvas |  |
| `canvas_commands` | sawit-canvas |  |
| `canvas_delete` | sawit-canvas |  |
| `canvas_design_guide` | sawit-canvas |  |
| `canvas_export` | sawit-canvas |  |
| `canvas_get` | sawit-canvas |  |
| `canvas_handoff` | sawit-canvas |  |
| `canvas_lint` | sawit-canvas |  |
| `canvas_load` | sawit-canvas |  |
| `canvas_progress` | sawit-canvas |  |
| `canvas_screen_export` | sawit-canvas |  |
| `canvas_screen_get` | sawit-canvas |  |
| `canvas_screen_write` | sawit-canvas |  |
| `canvas_settings` | sawit-canvas |  |
| `canvas_starter_system` | sawit-canvas |  |
| `canvas_tabs` | sawit-canvas |  |
| `canvas_update` | sawit-canvas |  |
| `check_port` | sawit-databases, sawit-terminal |  |
| `create_database` | sawit-databases |  |
| `create_instance` | sawit-databases |  |
| `create_prompt` | sawit-prompts |  |
| `delete_prompt` | sawit-prompts |  |
| `describe_schema` | sawit-databases |  |
| `describe_table` | sawit-databases |  |
| `detect_databases` | sawit-databases |  |
| `diagnose_network` | sawit-terminal |  |
| `drop_database` | sawit-databases |  |
| `drop_relation` | sawit-databases |  |
| `edit_permission` | sawit-app |  |
| `execute_sql` | sawit-databases, sawit-terminal |  |
| `export_query` | sawit-databases |  |
| `export_table` | sawit-databases |  |
| `fill_prompt` | sawit-prompts |  |
| `get_prompt` | sawit-prompts |  |
| `git_branches` | sawit-git |  |
| `git_commit` | sawit-git |  |
| `git_diff` | sawit-git |  |
| `git_fetch` | sawit-git |  |
| `git_log` | sawit-git |  |
| `git_pull` | sawit-git |  |
| `git_push` | sawit-git |  |
| `git_show` | sawit-git |  |
| `git_stage` | sawit-git |  |
| `git_status` | sawit-git |  |
| `git_switch` | sawit-git |  |
| `git_unstage` | sawit-git |  |
| `import_csv` | sawit-databases |  |
| `install_dev_tools` | sawit-project |  |
| `install_mcp_server` | sawit-project |  |
| `install_sawit_skills` | sawit-project |  |
| `install_skill` | sawit-project |  |
| `instance_logs` | sawit-databases |  |
| `keymap_bind` | sawit-app |  |
| `keymap_list` | sawit-app |  |
| `keymap_unbind` | sawit-app |  |
| `launch_app` | sawit-terminal, sawit-browser-testing, sawit-app |  |
| `list_agent_accounts` | sawit-app |  |
| `list_config_files` | sawit-project |  |
| `list_connections` | sawit-databases |  |
| `list_databases` | sawit-databases |  |
| `list_dev_tools` | sawit-project |  |
| `list_instances` | sawit-databases |  |
| `list_mcp_servers` | sawit-project |  |
| `list_permissions` | sawit-app |  |
| `list_ports` | sawit-terminal, sawit-browser-testing |  |
| `list_postgres_binaries` | sawit-databases |  |
| `list_prompts` | sawit-prompts |  |
| `list_scripts` | sawit-terminal |  |
| `list_skills` | sawit-project |  |
| `list_ssh_hosts` | sawit-ssh |  |
| `local_explorer` | sawit-terminal |  |
| `local_explorer_request` | sawit-terminal |  |
| `open_agent_account` | sawit-app |  |
| `ping` | sawit-terminal |  |
| `probe_ssh_host` | sawit-ssh |  |
| `query` | sawit-databases, sawit-terminal, sawit-prompts, sawit-app |  |
| `refresh_materialized_view` | sawit-databases |  |
| `remove_command` | sawit-terminal |  |
| `remove_connection` | sawit-databases |  |
| `remove_instance` | sawit-databases |  |
| `remove_ssh_host` | sawit-ssh |  |
| `render_diagram` | sawit-canvas |  |
| `render_mermaid` | — | not taught: deprecated alias |
| `restart_instance` | sawit-databases |  |
| `restore_prompts` | sawit-prompts |  |
| `run` | sawit-terminal, sawit-app |  |
| `save_command` | sawit-terminal |  |
| `save_connection` | sawit-databases |  |
| `schema_diagram` | sawit-databases, sawit-canvas |  |
| `search_api` | sawit (code mode) |  |
| `search_files` | sawit-app |  |
| `shell_files_sync` | sawit-project |  |
| `ssh_exec` | sawit-ssh |  |
| `stack_check` | sawit-project |  |
| `stack_create` | sawit-project |  |
| `stack_options` | sawit-project |  |
| `start_instance` | sawit-databases |  |
| `stop_instance` | sawit-databases |  |
| `stop_processes` | sawit-terminal |  |
| `sync_agent_settings` | sawit-app |  |
| `terminal_clear` | sawit-terminal |  |
| `terminal_close` | sawit-terminal |  |
| `terminal_input` | sawit-terminal, sawit-ssh |  |
| `terminal_lock` | sawit-terminal |  |
| `terminal_pin` | sawit-terminal |  |
| `terminal_read` | sawit-terminal, sawit-ssh |  |
| `terminal_rename` | sawit-terminal |  |
| `terminal_run` | sawit-terminal |  |
| `terminal_split` | sawit-terminal |  |
| `truncate_table` | sawit-databases |  |
| `uninstall_dev_tools` | sawit-project |  |
| `uninstall_mcp_server` | sawit-project |  |
| `uninstall_skill` | sawit-project |  |
| `update_prompt` | sawit-prompts |  |
| `update_ssh_host` | sawit-ssh |  |
| `upgrade_dev_tools` | sawit-project |  |
| `use_agent_account` | sawit-app |  |
| `write_env` | sawit-databases |  |

## The `sawit` command

| Command | Taught by |
|---|---|
| `sawit <path>, -n, -r` | sawit-app |
| `sawit browse / intip` | sawit-browser-testing |
| `sawit new / semai` | sawit-project |
| `sawit <db> [table] [filter or SQL]` | sawit-databases |
| `sawit list [db, instances, ports, connections, tools, mcp, skills, config, keys, explorer]` | sawit (router); each list in its skill |
| `sawit bakar ports / instances` | sawit-terminal, sawit-databases |
| `sawit nyalakan [--ulang]` | sawit-databases |
| `sawit panen, backup, tanam` | sawit-databases |
| `sawit pasang, pupuk, cabut` | sawit-project |
| `sawit skills install / remove` | sawit-project |
| `sawit github token` | sawit-git (GitHub MCP server via the MCP page) |
| `sawit permissions [add, move, remove]` | sawit-app |
| `sawit shell [upload, download]` | sawit-project |
| `sawit prompts backup / restore` | sawit-prompts |
| `sawit account list, use, open, usage, status, add…` | sawit-app |

## Gaps and follow-ups

- Tools added after this commit need their skill text in the same commit (B11's rule): the test
  above fails until they have it. At commit time the tracks still adding tools are the canvas MCP
  (B4, B15), browser captures (D5: `browser_screenshot` options, `browser_record_*`) and any
  further Accounts tools (C1).
- Not checked: a fresh agent finishing one task per family with only the skills loaded (the
  backlog's "done when"); that needs real agent sessions.
