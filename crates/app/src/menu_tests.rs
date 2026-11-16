//! The shared menu component's Sawit patch (vendor/gpui-component/SAWIT_PATCH.md): a long item
//! is cut on one line with an ellipsis inside the menu, keeps its right padding, and the menu
//! never runs past the window.

use gpui_kit::component::menu::{PopupMenu, PopupMenuItem};
use gpui_kit::{
    Bounds, Context, Entity, IntoElement, ParentElement as _, Pixels, Render, Styled as _,
    TestAppContext, VisualTestContext, Window, div, px, size,
};

/// A window's root that shows one menu at its content's width, as an anchored menu is.
struct Holder(Entity<PopupMenu>);

impl Render for Holder {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().flex().items_start().child(self.0.clone())
    }
}

/// A session title as long as the ones the user saw cut mid-letter (item 15, case A).
const LONG: &str = "Claude Code · ◑ FormValiditySummary card design review for the checkout page";

/// A window `width` wide holding a menu (`max` wide at most) with a short item and [`LONG`],
/// drawn; the bounds of the two labels.
fn draw(cx: &mut TestAppContext, width: f32, max: Option<f32>) -> (Bounds<Pixels>, Bounds<Pixels>) {
    cx.update(gpui_kit::init);
    let window = cx.open_window(size(px(width), px(400.)), move |window, cx| {
        Holder(PopupMenu::build(window, cx, move |menu, _, _| {
            let menu = menu
                .item(PopupMenuItem::new("Copy"))
                .item(PopupMenuItem::new(LONG));
            match max {
                Some(max) => menu.max_w(px(max)),
                None => menu,
            }
        }))
    });
    let mut cx = VisualTestContext::from_window(window.into(), cx);
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    let short = cx
        .debug_bounds("popup-menu-label-0")
        .expect("the short label");
    let long = cx
        .debug_bounds("popup-menu-label-1")
        .expect("the long label");
    (short, long)
}

#[gpui_kit::test]
fn a_long_item_is_cut_inside_its_menu(cx: &mut TestAppContext) {
    let (short, long) = draw(cx, 900., Some(236.));
    // Both labels start at the same place; the long one stops where the short one's row does,
    // within the 236 px menu, its padding kept.
    assert_eq!(short.origin.x, long.origin.x);
    assert!(long.size.width <= px(236.) - px(16.), "{long:?}");
    assert!(
        long.size.width > px(100.),
        "it still shows most of the title: {long:?}"
    );
}

#[gpui_kit::test]
fn a_menu_is_never_wider_than_its_window(cx: &mut TestAppContext) {
    // No max of its own: 500 px by default, and here the window is only 300 px wide.
    let (_, long) = draw(cx, 300., None);
    assert!(long.right() <= px(300.) - px(8.), "{long:?}");
}

#[gpui_kit::test]
fn a_short_menu_fits_its_content(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let window = cx.open_window(size(px(900.), px(400.)), |window, cx| {
        Holder(PopupMenu::build(window, cx, |menu, _, _| {
            menu.item(PopupMenuItem::new("Copy"))
        }))
    });
    let mut cx = VisualTestContext::from_window(window.into(), cx);
    cx.update(|window, cx| window.draw(cx).clear(cx));
    let label = cx.debug_bounds("popup-menu-label-0").expect("the label");
    assert!(
        label.size.width < px(200.),
        "content-fit, not the 500 px cap: {label:?}"
    );
}

// ---- B16: one menu look, app-wide -------------------------------------------------------------

use gpui_kit::component::menu::{MENU_ICON_SLOT, MENU_ROW_HEIGHT};

/// A menu as a section header, two items with icons (one destructive) and a separator, drawn in a
/// 900 px window; the bounds of the two labels and the header.
fn draw_look(cx: &mut TestAppContext) -> (Bounds<Pixels>, Bounds<Pixels>, Bounds<Pixels>) {
    cx.update(gpui_kit::init);
    let window = cx.open_window(size(px(900.), px(400.)), |window, cx| {
        Holder(PopupMenu::build(window, cx, |menu, _, _| {
            menu.label("Build in this project")
                .item(PopupMenuItem::new("Copy Prompt").icon(crate::menu_icons::COPY))
                .separator()
                .item(
                    PopupMenuItem::new("Delete")
                        .icon(crate::menu_icons::DELETE)
                        .danger(true),
                )
        }))
    });
    let mut cx = VisualTestContext::from_window(window.into(), cx);
    cx.update(|window, cx| window.draw(cx).clear(cx));
    let header = cx.debug_bounds("popup-menu-header-0").expect("the header");
    let copy = cx.debug_bounds("popup-menu-label-1").expect("Copy Prompt");
    let delete = cx.debug_bounds("popup-menu-label-3").expect("Delete");
    (header, copy, delete)
}

/// Rows are the app's one height, the labels line up after the icon slot, and the section header
/// starts where the labels do.
#[gpui_kit::test]
fn menu_rows_are_28_px_and_labels_line_up(cx: &mut TestAppContext) {
    let (header, copy, delete) = draw_look(cx);
    assert_eq!(MENU_ROW_HEIGHT, px(28.));
    assert_eq!(copy.origin.x, delete.origin.x);
    // Menu padding 4, item padding 8, the 16 px slot and the 8 px gap.
    assert_eq!(
        copy.origin.x,
        px(4.) + px(8.) + MENU_ICON_SLOT + px(8.),
        "{copy:?}"
    );
    // Two rows and a 1 px line with 4 px either side between the two labels' tops.
    assert_eq!(
        delete.origin.y - copy.origin.y,
        px(28.) + px(9.),
        "{copy:?} {delete:?}"
    );
    assert!(
        header.size.height < copy.size.height,
        "the header's text is smaller"
    );
}

/// Every item in a menu shows a picture; a separator or a header needs none.
#[test]
fn an_item_without_an_icon_has_no_picture() {
    assert!(!PopupMenuItem::new("Copy").has_picture());
    assert!(
        PopupMenuItem::new("Copy")
            .icon(crate::menu_icons::COPY)
            .has_picture()
    );
    assert!(PopupMenuItem::new("Phone").checked(true).has_picture());
    assert!(
        PopupMenuItem::new("Claude Code")
            .leading(|_, _| div())
            .has_picture()
    );
    assert!(PopupMenuItem::separator().has_picture());
    assert!(PopupMenuItem::label("Section").has_picture());
}

/// The builders that make a menu item from a label: each call must carry an icon (`.icon(…)`, a
/// `.leading(…)` logo, or `.checked(…)` in a choice list), or a `// no icon: <reason>` comment on
/// the lines just above it.
const ITEM_CALLS: &[&str] = &[
    "PopupMenuItem::new(",
    "PopupMenuItem::link(",
    "PopupMenuItem::submenu(",
    "PopupMenuItem::element(",
    "menus::keyed(",
];

/// The `PopupMenu` builder methods that add an item with no way to give it an icon: use
/// `.item(PopupMenuItem::new(…).icon(…))` or `.menu_with_icon(…)` instead.
const ICONLESS_BUILDERS: &[&str] = &[
    ".menu(",
    ".submenu(",
    ".menu_with_disabled(",
    ".menu_with_enable(",
    ".link_with_disabled(",
];

/// Files whose menus haven't been given their icons yet (B16 goes through them area by area).
const PENDING: &[&str] = &[];

/// `text` from byte `at` (an opening parenthesis) to just past its matching close.
fn balanced(text: &str, at: usize) -> usize {
    let mut depth = 0;
    let mut in_str = false;
    let bytes = text.as_bytes();
    let mut i = at;
    while i < bytes.len() {
        let c = bytes[i];
        if in_str {
            if c == b'\\' {
                i += 1;
            } else if c == b'"' {
                in_str = false;
            }
        } else if c == b'"' {
            in_str = true;
        } else if c == b'(' || c == b'{' || c == b'[' {
            depth += 1;
        } else if c == b')' || c == b'}' || c == b']' {
            depth -= 1;
            if depth == 0 {
                return i + 1;
            }
        }
        i += 1;
    }
    bytes.len()
}

/// The method chain after a call that ends at `end`: `.icon(…)`, `.on_click(…)` and so on, as
/// one string.
fn chain(text: &str, mut end: usize) -> String {
    let mut out = String::new();
    loop {
        let rest = &text[end..];
        let trimmed = rest.trim_start();
        // Skip line comments between the calls.
        if trimmed.starts_with("//") {
            let skip = rest.len() - trimmed.len();
            end += skip + trimmed.find('\n').unwrap_or(trimmed.len());
            continue;
        }
        if !trimmed.starts_with('.') {
            return out;
        }
        let start = end + (rest.len() - trimmed.len());
        let Some(open) = text[start..].find('(') else {
            return out;
        };
        let name = &text[start + 1..start + open];
        if !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
            return out;
        }
        let close = balanced(text, start + open);
        out.push_str(&text[start..close]);
        end = close;
    }
}

/// The app's source files outside tests, with their text cut before a `#[cfg(test)]` module.
fn sources() -> Vec<(String, String)> {
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut out = Vec::new();
    let mut dirs = vec![root.clone()];
    while let Some(dir) = dirs.pop() {
        for entry in std::fs::read_dir(&dir).unwrap().flatten() {
            let path = entry.path();
            let name = path.file_name().unwrap().to_string_lossy().to_string();
            if path.is_dir() {
                if name != "tests" {
                    dirs.push(path);
                }
                continue;
            }
            // `._` files are macOS's metadata beside sources on a non-Apple disk (exFAT).
            if !name.ends_with(".rs")
                || name.starts_with("._")
                || name == "tests.rs"
                || name.ends_with("_tests.rs")
            {
                continue;
            }
            let text = std::fs::read_to_string(&path).unwrap();
            let text = match text.find("#[cfg(test)]\nmod tests") {
                Some(at) => text[..at].to_owned(),
                None => text,
            };
            let rel = path
                .strip_prefix(&root)
                .unwrap()
                .to_string_lossy()
                .to_string();
            out.push((rel, text));
        }
    }
    out
}

/// Whether one of the three lines above byte `at` says why the item has no icon.
fn explained(text: &str, at: usize) -> bool {
    text[..at]
        .lines()
        .rev()
        .take(3)
        .any(|line| line.contains("// no icon:"))
}

/// The way the assets test fails on an icon that isn't embedded: every menu item with a label
/// has an icon, or says why not.
#[test]
fn every_menu_item_has_an_icon() {
    let mut missing = Vec::new();
    let mut seen = 0;
    for (file, text) in sources() {
        if PENDING.contains(&file.as_str()) {
            continue;
        }
        let line = |at: usize| text[..at].lines().count();
        for call in ITEM_CALLS {
            for (at, _) in text.match_indices(call) {
                seen += 1;
                let end = balanced(&text, at + call.len() - 1);
                let chain = chain(&text, end);
                let pictured = [".icon(", ".leading(", ".checked("]
                    .iter()
                    .any(|m| chain.contains(m));
                if !pictured && !explained(&text, at) {
                    missing.push(format!("{file}:{} {call}…)", line(at)));
                }
            }
        }
        if !text.contains("PopupMenu") && !text.contains("menu: PopupMenu") {
            continue;
        }
        for builder in ICONLESS_BUILDERS {
            for (at, _) in text.match_indices(builder) {
                // A closure is a floating bar's menu button (`BarButton::menu`), not an item.
                let arg = text[at + builder.len()..].trim_start();
                if arg.starts_with("move") || arg.starts_with('|') {
                    continue;
                }
                if !explained(&text, at) {
                    missing.push(format!("{file}:{} {builder}…) has no icon", line(at)));
                }
            }
        }
    }
    assert!(seen > 20, "found the menus: {seen}");
    assert!(
        missing.is_empty(),
        "menu items with no icon (add .icon(crate::menu_icons::…), or `// no icon: <why>`):\n{}",
        missing.join("\n")
    );
}
