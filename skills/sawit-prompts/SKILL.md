---
name: sawit-prompts
description: Sawit's prompt library: saved prompts with {{variables}}, found by words or #tags, filled in and opened for the user, and backed up to and restored from the user's private GitHub repository. Use it when the user asks to save, find, reuse or fill a prompt, or to back up or sync their prompts between Macs (list_prompts, fill_prompt, backup_prompts, restore_prompts). Works without the app.
---

# Prompts with Sawit

The prompt library is the user's: saved prompts with a title, tags and `{{variables}}` filled in each time. The tools work without the app, and the Prompts tab follows what they change.

## Find and use a prompt

1. `list_prompts` with `query` (words, or `#tag`) lists matching prompts with their ids, tags and variables, and ends with how the library syncs with GitHub.
2. `get_prompt` with `id` reads one whole.
3. `fill_prompt` with `id` and `values` (`{"language": "Rust"}`) returns the text with its variables filled; it says which variables were left empty.
4. `app_open_prompt` with `id` opens it in a tab for the user to read or fill (needs the app).

## Keep the library

- `create_prompt` with `title`, `body` (mark variables as `{{name}}`) and `tags`.
- `update_prompt` with `id` and what changes (`title`, `body`, `tags`, `favorite`).
- `delete_prompt` deletes for good and needs `confirm` equal to the id: ask first.

## GitHub backup and sync

The library backs up to a private repository of the user's (`sawit-prompts` unless they chose another), one Markdown file per prompt, with Sawit's GitHub sign-in: only the user signs in.

1. `backup_prompts` with `dry_run: true` first: what would be sent, deleted on GitHub, and any lines that look like secrets.
2. `backup_prompts` sends the prompts changed here and deletes on GitHub those deleted here since the last sync. A prompt whose text looks like it holds a secret (an API key, a token) holds the backup back and is listed: ask the user, then pass `allow_secrets: true` only if they agree.
3. `restore_prompts` brings in what changed on GitHub (another Mac's edits, new prompts) and deletes here the prompts deleted there; it keeps local use counts and never overwrites a prompt changed here.
4. A prompt changed on both sides is listed, never overwritten. Ask the user which side wins, then name its id in `overwrite`.

## Without the sawit MCP server

Tell the user, as the `sawit` skill says. In a shell, `sawit prompts backup` and `sawit prompts restore` do the GitHub sync (they ask first; `-y` answers yes, and lets a backup send secret-looking lines).
