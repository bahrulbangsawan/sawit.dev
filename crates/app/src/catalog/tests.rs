//! The catalog pieces' tests: a page's rows and the size kept per list across launches, the
//! pager's and a list line's layout from wide to narrow, and the home folder shown as `~`.

use gpui_kit::{
    Bounds, Context, InteractiveElement as _, IntoElement, ParentElement as _, Pixels, Render,
    Styled as _, TestAppContext, VisualTestContext, Window, div, px, size,
};

use super::list::NAME_MIN;
use super::paging::save_size;
use super::{
    PageSize, Paging, TILE, columns, keep_page_sizes, page_of, pager, saved_size, tilde_in,
};

fn paging(page: usize, size: PageSize) -> Paging {
    Paging { page, size }
}

/// The size a catalog list starts at.
fn saved(list: &str, cx: &gpui_kit::App) -> PageSize {
    saved_size(list, &PageSize::CHOICES, PageSize::DEFAULT, cx)
}

/// 23 rows, 8 a page: three pages, the last of 7; a page past the end shows the last one, and
/// All shows every row on one page.
#[test]
fn a_page_holds_the_rows_its_size_says() {
    let rows: Vec<usize> = (0..23).collect();
    let (shown, at) = page_of(rows.clone(), paging(0, PageSize::Rows(8)));
    assert_eq!((shown, at.page), ((0..8).collect(), 0));
    let (shown, at) = page_of(rows.clone(), paging(9, PageSize::Rows(8)));
    assert_eq!((shown, at.page), ((16..23).collect(), 2));
    let (shown, at) = page_of(rows.clone(), paging(1, PageSize::Rows(16)));
    assert_eq!((shown, at.page), ((16..23).collect(), 1));
    let (shown, at) = page_of(rows.clone(), paging(2, PageSize::All));
    assert_eq!((shown, at), (rows, paging(0, PageSize::All)));
    assert_eq!(PageSize::Rows(8).pages(23), 3);
    assert_eq!(PageSize::Rows(64).pages(23), 1);
    assert_eq!(PageSize::All.pages(0), 1);
    let (shown, at) = page_of(Vec::<usize>::new(), paging(3, PageSize::Rows(8)));
    assert_eq!((shown.len(), at.page), (0, 0));
}

/// A new size shows the page with the selected row on it; without one, the page with the row
/// that was first on the page before.
#[test]
fn a_new_size_keeps_the_selected_row_or_the_first_row_shown() {
    // Rows 16–23 shown, row 20 selected.
    let at = paging(2, PageSize::Rows(8));
    assert_eq!(at.resized(PageSize::Rows(16), Some(20)).page, 1);
    assert_eq!(at.resized(PageSize::Rows(32), Some(20)).page, 0);
    assert_eq!(
        at.resized(PageSize::All, Some(20)),
        paging(0, PageSize::All)
    );
    // No selection: row 16 stays in sight.
    assert_eq!(at.resized(PageSize::Rows(16), None).page, 1);
    assert_eq!(at.resized(PageSize::Rows(32), None).page, 0);
    // From every row on one page, back to pages: the selected row's, else the first.
    let all = paging(0, PageSize::All);
    assert_eq!(all.resized(PageSize::Rows(8), Some(20)).page, 2);
    assert_eq!(all.resized(PageSize::Rows(8), None).page, 0);
    // The keyboard turns to the selected row's page at any size.
    let mut at = paging(0, PageSize::Rows(16));
    at.show(20);
    assert_eq!(at.page, 1);
    let mut at = paging(0, PageSize::All);
    at.show(20);
    assert_eq!(at.page, 0);
}

/// A size picked is kept per list in the file, and read back at the next launch; a list never
/// resized, a file that isn't JSON, or a size the Show menu doesn't offer starts at the default.
#[gpui_kit::test]
fn the_size_picked_is_kept_per_list_across_launches(cx: &mut TestAppContext) {
    let dir = crate::testing::scratch_dir("page-sizes");
    let file = dir.join("page-sizes.json");
    cx.update(|cx| {
        keep_page_sizes(file.clone(), cx);
        assert_eq!(saved("tools", cx), PageSize::DEFAULT);
        save_size("tools", PageSize::Rows(32), cx);
        save_size("mcp", PageSize::All, cx);
        assert_eq!(Paging::new("tools", cx), paging(0, PageSize::Rows(32)));
    });
    cx.run_until_parked();
    let text = std::fs::read_to_string(&file).unwrap();
    assert!(
        text.contains(r#""tools": {"#) && text.contains(r#""mcp": "all""#),
        "{text}"
    );
    // The next launch.
    cx.update(|cx| {
        keep_page_sizes(file.clone(), cx);
        assert_eq!(saved("tools", cx), PageSize::Rows(32));
        assert_eq!(saved("mcp", cx), PageSize::All);
        assert_eq!(saved("skills", cx), PageSize::DEFAULT);
    });
    for broken in [
        "not json",
        r#"{"tools": {"rows": 0}}"#,
        r#"{"tools": {"rows": 10}}"#,
    ] {
        std::fs::write(&file, broken).unwrap();
        cx.update(|cx| {
            keep_page_sizes(file.clone(), cx);
            assert_eq!(saved("tools", cx), PageSize::DEFAULT, "{broken}");
        });
    }
}

/// Without the file (the app's tests), a size picked lasts for the run and nothing is written.
#[gpui_kit::test]
fn without_the_file_a_size_lasts_for_the_run(cx: &mut TestAppContext) {
    cx.update(|cx| {
        assert_eq!(saved("tools", cx), PageSize::DEFAULT);
        save_size("tools", PageSize::Rows(16), cx);
        assert_eq!(saved("tools", cx), PageSize::Rows(16));
    });
}

/// A pager over `total` rows whose parts the tests can find.
struct Footer {
    total: usize,
    at: Paging,
    selected: Option<usize>,
    /// What the pager handed back last.
    got: std::rc::Rc<std::cell::Cell<Option<Paging>>>,
}

impl Footer {
    fn new(total: usize, at: Paging) -> Self {
        Self {
            total,
            at,
            selected: None,
            got: Default::default(),
        }
    }
}

impl Render for Footer {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let got = self.got.clone();
        div().size_full().child(pager(
            "test",
            self.at,
            self.total,
            "tools",
            self.selected,
            move |paging, _, _| got.set(Some(*paging)),
            cx,
        ))
    }
}

/// Show, then 16 from the menu (by the keyboard, as a click would): the pager hands back 16
/// rows a page on the page holding the selected row, and keeps 16 for the list.
#[gpui_kit::test]
fn picking_a_size_keeps_the_selected_row_in_sight(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let got = std::rc::Rc::default();
    let window = cx.open_window(size(px(900.), px(400.)), {
        let got = std::rc::Rc::clone(&got);
        move |_, _| Footer {
            selected: Some(20),
            got,
            ..Footer::new(23, paging(2, PageSize::Rows(8)))
        }
    });
    let mut cx = VisualTestContext::from_window(window.into(), cx);
    cx.run_until_parked();
    let show = cx.debug_bounds("test-page-size").expect("no Show menu");
    cx.simulate_click(show.center(), Default::default());
    cx.run_until_parked();
    cx.simulate_keystrokes("down down enter");
    cx.run_until_parked();
    let got: &std::cell::Cell<Option<Paging>> = &got;
    assert_eq!(got.get(), Some(paging(1, PageSize::Rows(16))));
    cx.update(|_, cx| assert_eq!(saved("test", cx), PageSize::Rows(16)));
}

/// Where a pager `width` wide puts the count with its Show menu, and the page buttons.
fn lay_out_pager(
    width: f32,
    total: usize,
    cx: &mut TestAppContext,
) -> (
    Bounds<Pixels>,
    Option<Bounds<Pixels>>,
    Option<Bounds<Pixels>>,
) {
    cx.update(gpui_kit::init);
    let at = paging(0, PageSize::DEFAULT);
    let window = cx.open_window(size(px(width), px(200.)), |_, _| Footer::new(total, at));
    let mut cx = VisualTestContext::from_window(window.into(), cx);
    cx.run_until_parked();
    (
        cx.debug_bounds("test-count")
            .expect("the count isn't drawn"),
        cx.debug_bounds("test-page-size"),
        cx.debug_bounds("test-pages"),
    )
}

#[gpui_kit::test]
fn a_wide_pager_keeps_count_size_and_pages_on_one_line(cx: &mut TestAppContext) {
    let (count, size, pages) = lay_out_pager(900., 23, cx);
    let (size, pages) = (size.expect("no Show menu"), pages.expect("no page buttons"));
    assert_eq!(at(pages.center().y), at(count.center().y));
    assert!(count.contains(&size.center()), "{count:?} {size:?}");
    assert!(count.right() < pages.left());
    assert!(at(pages.right()) <= 900. - 16., "{pages:?}");
}

/// The narrowest list the window has, with the sidebar open (720 wide, less the sidebar's
/// 240 and the page's padding), shows the whole pager: what doesn't fit beside the count
/// wraps under it, on the right.
#[gpui_kit::test]
fn the_narrowest_pager_wraps_the_page_buttons_and_shows_them_all(cx: &mut TestAppContext) {
    for width in [430., 330.] {
        let (count, size, pages) = lay_out_pager(width, 23, cx);
        let (size, pages) = (size.expect("no Show menu"), pages.expect("no page buttons"));
        assert!(count.contains(&size.center()), "{count:?} {size:?}");
        assert!(at(count.right()) <= width, "{width}: {count:?}");
        assert!(at(pages.right()) <= width, "{width}: {pages:?}");
        assert!(
            pages.top() >= count.bottom() || pages.left() > count.right(),
            "{width}: {count:?} {pages:?}"
        );
    }
}

/// Fewer rows than the smallest page: no page buttons, and no size to pick.
#[gpui_kit::test]
fn a_short_list_has_no_size_or_pages_to_pick(cx: &mut TestAppContext) {
    let (_, size, pages) = lay_out_pager(900., 8, cx);
    assert!(size.is_none() && pages.is_none(), "{size:?} {pages:?}");
}

/// The home folder by its path's parts, not its text: `/Users/bobby` isn't in `/Users/bob`.
#[test]
fn only_the_home_folder_is_shown_as_tilde() {
    use std::path::Path;
    let home = Some(Path::new("/Users/bob"));
    assert_eq!(
        tilde_in(Path::new("/Users/bob/.cursor/mcp.json"), home),
        "~/.cursor/mcp.json"
    );
    assert_eq!(
        tilde_in(Path::new("/Users/bobby/x"), home),
        "/Users/bobby/x"
    );
    assert_eq!(tilde_in(Path::new("/opt/x"), None), "/opt/x");
}

/// A line of the list whose parts the tests can find.
struct Line;

impl Render for Line {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full().child(columns(
            None,
            div().size(px(TILE)).flex_none(),
            div().debug_selector(|| "name".into()).h(px(40.)),
            div()
                .debug_selector(|| "status".into())
                .w(px(140.))
                .h(px(22.)),
            div()
                .debug_selector(|| "actions".into())
                .w(px(120.))
                .h(px(28.)),
        ))
    }
}

/// Where a line `width` wide puts its name, status and actions.
fn lay_out(width: f32, cx: &mut TestAppContext) -> [Bounds<Pixels>; 3] {
    cx.update(gpui_kit::init);
    let window = cx.open_window(size(px(width), px(400.)), |_, _| Line);
    let mut cx = VisualTestContext::from_window(window.into(), cx);
    cx.run_until_parked();
    ["name", "status", "actions"].map(|selector| {
        cx.debug_bounds(selector)
            .unwrap_or_else(|| panic!("{selector} isn't drawn"))
    })
}

fn at(edge: Pixels) -> f32 {
    f32::from(edge).round()
}

#[gpui_kit::test]
fn a_wide_list_keeps_name_status_and_actions_on_one_line(cx: &mut TestAppContext) {
    let [name, status, actions] = lay_out(1000., cx);
    assert_eq!(at(status.top()), at(name.top()));
    assert_eq!(at(actions.top()), at(name.top()));
    assert!(name.size.width > px(NAME_MIN), "{name:?}");
    assert!(name.right() < status.left() && status.right() < actions.left());
    assert_eq!(at(actions.right()), 1000.);
}

/// The window's narrowest page, with the sidebar open, still shows every column: what
/// doesn't fit beside the name wraps under it, the actions on the right.
#[gpui_kit::test]
fn a_narrower_list_wraps_the_actions_under_the_name(cx: &mut TestAppContext) {
    let [name, status, actions] = lay_out(700., cx);
    assert_eq!(at(status.top()), at(name.top()));
    assert!(actions.top() >= name.bottom(), "{name:?} {actions:?}");
    assert_eq!(at(actions.right()), 700.);
}

#[gpui_kit::test]
fn a_narrow_list_wraps_status_and_actions_under_the_name(cx: &mut TestAppContext) {
    let [name, status, actions] = lay_out(400., cx);
    assert!(status.top() >= name.bottom(), "{name:?} {status:?}");
    assert_eq!(at(status.left()), at(name.left()));
    assert!(actions.top() >= status.bottom(), "{status:?} {actions:?}");
    assert_eq!(at(actions.right()), 400.);
    assert!(name.right() <= px(400.), "{name:?}");
}
