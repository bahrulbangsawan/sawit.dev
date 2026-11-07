---
name: sawit-ssh
description: Servers over SSH through Sawit: the saved hosts and ~/.ssh/config, a check that one answers, one command with its exit code, or an interactive session in a Sawit terminal the user watches. Use it when a task runs something on a remote machine, reads a server's logs or deploys by hand over SSH. Saved passwords stay in the Keychain (list_ssh_hosts, ssh_exec, app_ssh_connect).
---

# SSH with Sawit

Sawit keeps a list of servers (its own saved hosts and the hosts in `~/.ssh/config`) and runs SSH for you, with saved passwords answered from the macOS Keychain. Use it instead of an `ssh` you start where the user can't see it.

## Run something on a server

1. `list_ssh_hosts` lists the hosts, saved and from `~/.ssh/config`, with their ids, user, port and group.
2. `probe_ssh_host` with `host` checks that it answers, and how (key, password, jump host), before you rely on it.
3. `ssh_exec` with `host` and `command` runs one non-interactive command (`timeout_secs`) and returns its exit code and output. It never waits for a password prompt the Keychain can't answer.
4. For anything interactive or long (a REPL, `tail -f`, a deploy you want the user to watch), `app_ssh_connect` with `host` opens a session in Sawit's terminal (`new_session` for another one) and returns the tab and pane: drive it with `terminal_input` and read it with `terminal_read` (see `sawit-terminal`). It needs the app.

## Keep the list

- `add_ssh_host` saves a server (`name`, `host`, `user`, `port`, `identity_file`, `password`, `jump`, `forwards`, `group`, `remote_dir`, `startup`). A password goes to the Keychain, never to a file. `update_ssh_host` changes one by id.
- `remove_ssh_host` deletes a saved host and needs `confirm`: ask first. Hosts from `~/.ssh/config` stay in that file.

## Care

- Ask before you run anything that changes a server (restarts, deploys, deletes, package upgrades). Reading logs and status needs no question.
- Don't print or store a password; the Keychain answers for saved hosts.

## Without the sawit MCP server

Tell the user, as the `sawit` skill says, and use `ssh` in your shell for read-only checks; ask before you change anything on the server.
