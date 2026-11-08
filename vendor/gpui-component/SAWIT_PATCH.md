# gpui-component 0.8.1 — Sawit patch

Copy of `gpui-component` 0.8.1 from crates.io (Apache-2.0), used via `[patch.crates-io]` in the
workspace `Cargo.toml`. Changes (search for "Sawit patch"):

- `src/menu/popup_menu.rs`: a menu item's label stays on one line and is cut with an ellipsis
  when the menu is too narrow for it, keeping the item's right padding and its trailing shortcut;
  a cut label shows the whole of it in a tooltip (`render_label`, `label_room`). A custom item
  (`PopupMenuItem::element`) may shrink, so its own `truncate()` works. The menu fits its content
  up to its `max_w` (500 px by default) and never runs past the window less 8 px each side
  (`max_width`). Before, a long label ran out of the menu's right edge, cut mid-letter.
- `src/menu/popup_menu.rs`, B16's one menu look: 28 px rows (`MENU_ROW_HEIGHT`), 4 px menu
  padding with rows flush, each item's picture in a 16 px slot (`MENU_ICON_SLOT`, the icon at
  14 px in the muted icon colour) 8 px before its label, a 1 px separator with 4 px above and
  below, shortcuts muted on the right. A `label` is a section header (small, semibold, muted,
  room above), never a dimmed item. `PopupMenuItem::danger` draws a destructive item's icon and
  label in the danger colour; `PopupMenuItem::leading` draws a picture (an agent's colour logo)
  in the icon's slot; `has_picture` and `PopupMenu::items` let tests check every item has one.
- `src/menu/popup_menu.rs`: an item drawn from elements (`PopupMenuItem::element`: the canvas's
  keyed commands, its option rows, Send to Agent) keeps to one line (`whitespace_nowrap`), so it is
  the same 28 px as a labelled item: the canvas's View menu drew its keyed items taller than the
  rest, which made its spacing look uneven.
- `src/tooltip.rs`: a text tooltip is at most 360 px wide and wraps, so a long one (a whole
  session title, a path, an error) never runs off the window on one line. It blocks the mouse
  (`occlude`), as menus do, so Sawit's native views (the browser's page, live web frames) see it
  over them and step back instead of hiding it.
- `src/highlighter/syntect_backend.rs`, `src/highlighter/mod.rs`: `highlighter::syntax_set()`
  hands out the syntect syntax set the Markdown code blocks use, so Sawit's own editors'
  highlighter (`crates/app/src/highlight`) shares it instead of loading a second copy (several MB
  each, B6).

Upstream has no option for these. To drop the patch, delete this folder and the
`[patch.crates-io]` entry. Sawit's tests of it: `crates/app/src/menu_tests.rs`.
