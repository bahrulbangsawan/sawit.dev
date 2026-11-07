---
name: sawit-git
description: Git through Sawit's MCP server, the same repository state the user sees in Sawit's Git tab: status, diffs, history, staging, commits, branches, fetch, pull and push, and the GitHub page. Use it when you inspect or commit changes, switch or create a branch, sync with the remote, or show the user the commit graph or a pull request. Works without the app, except the pages it opens (git_status, git_diff, git_commit, git_push).
---

# Git with Sawit

Sawit's git tools run git on the repository that holds the folder you pass as `repo` (an absolute path; any folder inside the repository works). They work without the app, and what they change shows at once in the user's Git tab.

## Look before you change

1. `git_status` with `repo`: the branch, its upstream with ahead and behind, conflicts, and staged, unstaged and untracked files.
2. `git_diff` (`staged` for what is staged; `path` for one file); `git_log` (`limit`, `path`) and `git_show` (`hash`) for history; `git_branches` for local and remote branches.

## Commit

1. `git_stage` with `paths` (relative to the repository root), or none for every change; `git_unstage` takes them back.
2. `git_commit` with `message`. `amend: true` rewrites the last commit (an empty message keeps its message): only when the user asked, and never on a commit already pushed.
3. Commit only what the user asked you to commit. Don't stage files you didn't touch.

## Branches and the remote

- `git_switch` with `branch`; `create: true` makes it from the current commit. A switch with changes git can't carry is refused: say so rather than stashing on your own.
- `git_fetch` fetches every remote; `git_pull` fast-forwards only (a diverged branch is left for the user to merge); `git_push` pushes the current branch, setting its upstream on origin the first time, and never forces. Push only when the user asked.
- They use the user's own git credentials first, then Sawit's GitHub sign-in for a github.com remote over HTTPS. Only the user signs in to GitHub: `app_state`'s `github` says whether they have.

## Show it to the user

- `app_show_panel` with `panel`: git shows the Git tab; `app_open_page` with `page`: git_graph shows the commit graph, github the GitHub page (pull requests, issues, the user's repositories). Signed out, the GitHub page shows the user its sign-in card and the answer says to wait for them.
- For pull requests and issues themselves, use GitHub's MCP server when it is connected (the MCP page installs it on Sawit's sign-in).

## Without the sawit MCP server

Tell the user, as the `sawit` skill says, and use `git` in your shell the same way: look first, stage only what you changed, never force-push.
