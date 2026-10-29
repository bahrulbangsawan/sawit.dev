# gpui-base 0.8.1 — Sawit patch

Copy of `gpui-base` 0.8.1 from crates.io (Apache-2.0), used via
`[patch.crates-io]` in the workspace `Cargo.toml`. Changes (search for
"Sawit patch"):

- `src/input/base/element.rs`: line numbers are left-aligned (`{:<width$}`
  instead of `{:>width$}`), and the gutter reserves 2 digits minimum instead
  of 3.
  The code area has a 10px left margin after the gutter (`CODE_LEFT_MARGIN`,
  `LastLayout::gutter_width`); the gutter background and hitbox stop before it.
- `src/text/format/html.rs`: `<pre>` parses as a code block (language from a
  `language-x` / `lang-x` class) instead of a paragraph, so HTML previews get
  code block styling and actions (Run / Copy).
- `src/text/node.rs`: a code block with actions gets the same top padding as
  one with a language label, so the buttons don't cover its first line.
- `src/text/node.rs`: code block highlights are shared across re-parses
  (`SHARED_HIGHLIGHTS`, keyed by highlighter, language and code), so a
  Markdown preview re-highlights only the code blocks that changed, not every
  visible one on each debounced edit.

Upstream has no option for any of these. To drop the patch, delete this folder and
the `[patch.crates-io]` entry.
