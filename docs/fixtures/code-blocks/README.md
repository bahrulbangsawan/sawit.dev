# Code block test fixtures

Open each file in Sawit, switch to **Preview** (or Split), and check the list.
All runnable commands are harmless: `echo`, `pwd`, `ls`, and files under
`/tmp/sawit-fixture`.

| File | What it tests |
|---|---|
| `shells.md` | Every shell language shows **▶ Run + Copy**; each prints `OK <lang>` |
| `edge-cases.md` | `$` transcripts, multi-line, `\` continuations, heredocs, long first line, nesting, odd language names |
| `non-shell.md` | SQL, Rust, JSON… show **Copy only** (no Run) |
| `windows.md` | PowerShell/cmd: **Run on Windows only**, Copy only on macOS/Linux |
| `preview.html` | HTML `<pre>` blocks: language from `class`, Run/Copy, plain `<pre>` |
| `queries.sql` | SQL editor (open as code, run against a database) |
| `diagram.svg` | SVG preview still renders (no code blocks) |

## Checklist

1. **Buttons**: shell blocks show the language, ▶ and Copy in the top-right; other blocks show the language and Copy.
2. **No overlap**: the buttons never cover the first line of code (see the long first line in `edge-cases.md`).
3. **Run, terminal hidden**: hide the terminal (⌘J), click ▶. The terminal opens and the command runs.
4. **Run, herdr active**: open a herdr tab and make it active, click ▶. The command goes to a shell tab, not herdr.
5. **Run, no shell**: close every terminal tab, click ▶. A new shell tab opens and runs it.
6. **Run, exited shell**: type `exit` in the shell, click ▶. A new shell opens.
7. **Copy**: click Copy, paste somewhere. You get the exact block text, `$` prompts and output included.
8. **Two windows**: open two Sawit windows, click ▶ in one. Only that window's terminal runs it.
9. **Changelog**: open the Version / changelog page. Its code blocks (if any) get the same buttons.
