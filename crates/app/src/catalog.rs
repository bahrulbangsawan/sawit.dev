//! Pieces the catalog pages (Dev Tools, MCP) share: the toolbar (search,
//! filter tabs, menus), status pills, tags, logo tiles, the list's columns
//! and their header, its pager (with the rows a page holds, kept per list
//! across launches) and the empty state.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::*;
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_kit::component::pagination::Pagination;
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use serde::{Deserialize, Serialize};

actions!(
    catalog,
    [
        /// Moves the keyboard to the page's search field.
        FocusSearch,
        /// Selects the next row of the list.
        SelectNext,
        /// Selects the previous row of the list.
        SelectPrevious,
        /// Runs the selected row's main action: Install, Upgrade, Retry, or opens a server's
        /// sheet; in the MCP page's sheet, Install.
        Confirm
    ]
);

/// The catalog pages' keys. The list's plain keys act while the search field doesn't have
/// the keyboard (↩ there moves it to the first row), and the MCP page's while its sheet is
/// closed; in the sheet, ↩ installs. The New Project page's move through its cards, and ↩ picks
/// one.
pub fn init(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("cmd-f", FocusSearch, Some("ToolsPage")),
        KeyBinding::new("down", SelectNext, Some("ToolsPage && !Input")),
        KeyBinding::new("up", SelectPrevious, Some("ToolsPage && !Input")),
        KeyBinding::new("enter", Confirm, Some("ToolsPage && !Input")),
        KeyBinding::new("cmd-f", FocusSearch, Some("McpPage && !McpSheet")),
        KeyBinding::new("down", SelectNext, Some("McpPage && !Input && !McpSheet")),
        KeyBinding::new("up", SelectPrevious, Some("McpPage && !Input && !McpSheet")),
        KeyBinding::new("enter", Confirm, Some("McpPage && !Input")),
        KeyBinding::new("cmd-f", FocusSearch, Some("SkillsPage && !SkillsSheet")),
        KeyBinding::new(
            "down",
            SelectNext,
            Some("SkillsPage && !Input && !SkillsSheet"),
        ),
        KeyBinding::new(
            "up",
            SelectPrevious,
            Some("SkillsPage && !Input && !SkillsSheet"),
        ),
        KeyBinding::new("enter", Confirm, Some("SkillsPage && !Input")),
        KeyBinding::new("cmd-f", FocusSearch, Some("NewProjectPage")),
        KeyBinding::new("down", SelectNext, Some("NewProjectPage && !Input")),
        KeyBinding::new("up", SelectPrevious, Some("NewProjectPage && !Input")),
        KeyBinding::new("enter", Confirm, Some("NewProjectPage && !Input")),
    ]);
}

/// The row a step of ↑ or ↓ selects in a list of `len` rows, from `current`.
pub fn step(current: Option<usize>, len: usize, down: bool) -> Option<usize> {
    let last = len.checked_sub(1)?;
    Some(match (current, down) {
        (None, _) => 0,
        (Some(ix), true) => (ix + 1).min(last),
        (Some(ix), false) => ix.saturating_sub(1).min(last),
    })
}

/// How many rows a page of a list holds: a number of them, or every row on one page.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PageSize {
    Rows(usize),
    All,
}

impl PageSize {
    /// What the pager's Show menu offers.
    pub const CHOICES: [PageSize; 5] = [
        Self::Rows(8),
        Self::Rows(16),
        Self::Rows(32),
        Self::Rows(64),
        Self::All,
    ];

    /// A list's size until the user picks another.
    pub const DEFAULT: PageSize = Self::Rows(8);

    /// Rows on a full page of a list `total` long (at least one).
    fn rows(self, total: usize) -> usize {
        match self {
            Self::Rows(n) => n.max(1),
            Self::All => total.max(1),
        }
    }

    /// How many pages `total` rows fill (at least one).
    pub fn pages(self, total: usize) -> usize {
        total.div_ceil(self.rows(total)).max(1)
    }

    /// The page (from 0) that row `ix` is on.
    pub fn page_holding(self, ix: usize) -> usize {
        match self {
            Self::Rows(n) => ix / n.max(1),
            Self::All => 0,
        }
    }

    /// "8", or "All".
    fn label(self) -> String {
        match self {
            Self::Rows(n) => n.to_string(),
            Self::All => "All".into(),
        }
    }
}

/// Which page of a list is shown, from 0, and how many rows a page holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Paging {
    pub page: usize,
    pub size: PageSize,
}

impl Paging {
    /// The first page of list `list` (`tools`, `mcp`), at the size the user left it at.
    pub fn new(list: &str, cx: &App) -> Self {
        Self {
            page: 0,
            size: saved_size(list, &PageSize::CHOICES, PageSize::DEFAULT, cx),
        }
    }

    /// Turns to the page holding row `ix`, as selecting it with the keyboard does.
    pub fn show(&mut self, ix: usize) {
        self.page = self.size.page_holding(ix);
    }

    /// The same list at `size` rows a page, on the page holding row `keep` (the selected one)
    /// when there is one, else the row that was first on the page shown.
    pub fn resized(self, size: PageSize, keep: Option<usize>) -> Self {
        let first = match self.size {
            PageSize::Rows(n) => self.page * n.max(1),
            PageSize::All => 0,
        };
        Self {
            page: size.page_holding(keep.unwrap_or(first)),
            size,
        }
    }
}

/// The page size each list was left at, by list, and the file that keeps them
/// ([`keep_page_sizes`]).
#[derive(Default)]
struct PageSizes {
    file: Option<PathBuf>,
    sizes: BTreeMap<String, PageSize>,
    /// The file's text as last changed, which every write takes as it starts: two sizes picked
    /// in quick succession write the second, whichever write ends last.
    latest: Arc<Mutex<String>>,
}

/// One write of the page sizes at a time.
static WRITING: Mutex<()> = Mutex::new(());

impl Global for PageSizes {}

/// Reads the page sizes the lists were left at from `file` (`page-sizes.json` in the app's
/// folder), and keeps there the ones picked from now on. Startup does it once; without it, as in
/// tests, lists start at [`PageSize::DEFAULT`] and a size picked is kept for this run only.
pub fn keep_page_sizes(file: PathBuf, cx: &mut App) {
    let sizes = std::fs::read_to_string(&file)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default();
    cx.set_global(PageSizes {
        file: Some(file),
        sizes,
        latest: Arc::default(),
    });
}

/// The size list `list` was left at, if it is one of `choices`; else, for a list never resized
/// or a file edited by hand, `fallback`.
pub fn saved_size(list: &str, choices: &[PageSize], fallback: PageSize, cx: &App) -> PageSize {
    cx.try_global::<PageSizes>()
        .and_then(|saved| saved.sizes.get(list).copied())
        .filter(|size| choices.contains(size))
        .unwrap_or(fallback)
}

/// Keeps `size` as list `list`'s, for the pages opened from now on and the next launch.
fn save_size(list: &str, size: PageSize, cx: &mut App) {
    let saved = cx.default_global::<PageSizes>();
    saved.sizes.insert(list.to_string(), size);
    let Some(file) = saved.file.clone() else {
        return;
    };
    let Ok(text) = serde_json::to_string_pretty(&saved.sizes) else {
        return;
    };
    let latest = saved.latest.clone();
    *latest.lock().unwrap_or_else(|e| e.into_inner()) = text;
    cx.background_spawn(async move {
        let _writing = WRITING.lock().unwrap_or_else(|e| e.into_inner());
        let text = latest.lock().unwrap_or_else(|e| e.into_inner()).clone();
        if let Some(dir) = file.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = sawit_core::files::write_atomic(&file, text.as_bytes());
    })
    .detach();
}

/// `~/…` instead of the home folder's path.
pub fn tilde(path: &std::path::Path) -> String {
    tilde_in(
        path,
        std::env::var_os("HOME")
            .map(std::path::PathBuf::from)
            .as_deref(),
    )
}

fn tilde_in(path: &std::path::Path, home: Option<&std::path::Path>) -> String {
    sawit_core::recent::tildify(path, home)
}

/// Width of the status column, header and rows alike.
pub const STATUS_W: f32 = 200.;
/// Width of the actions column, header and rows alike.
pub const ACTIONS_W: f32 = 236.;
/// Side of a row's logo tile.
pub const TILE: f32 = 36.;
/// The narrowest the name column gets beside status and actions: narrower,
/// and they wrap under the name.
const NAME_MIN: f32 = 240.;

/// The rows on page `paging.page` of `rows`, and the paging with that page clamped to the last
/// one, for the pager.
pub fn page_of<T>(rows: Vec<T>, paging: Paging) -> (Vec<T>, Paging) {
    let total = rows.len();
    let page = paging.page.min(paging.size.pages(total) - 1);
    let per_page = paging.size.rows(total);
    let rows = rows
        .into_iter()
        .skip(page * per_page)
        .take(per_page)
        .collect();
    (rows, Paging { page, ..paging })
}

type OnPick = Rc<dyn Fn(&usize, &mut Window, &mut App)>;

/// The search field, with a magnifier and a clear button.
pub fn search(input: &Entity<InputState>, cx: &App) -> impl IntoElement + use<> {
    div().flex_1().min_w(px(180.)).max_w(px(340.)).child(
        Input::new(input)
            .small()
            .prefix(
                Icon::new(Lucide::Search)
                    .xsmall()
                    .text_color(cx.theme().muted_foreground),
            )
            .cleanable(true),
    )
}

/// Segmented tabs, "Installed 16", one picked; `on` gets the index.
pub fn tabs(
    id: &'static str,
    choices: Vec<(&'static str, usize)>,
    current: usize,
    on: impl Fn(&usize, &mut Window, &mut App) + 'static,
    cx: &App,
) -> Div {
    let on: OnPick = Rc::new(on);
    h_flex()
        .flex_none()
        .p(px(2.))
        .gap(px(2.))
        .rounded(cx.theme().radius)
        .border_1()
        .border_color(cx.theme().border)
        .bg(cx.theme().secondary)
        .children(
            choices
                .into_iter()
                .enumerate()
                .map(move |(ix, (label, n))| {
                    let on = on.clone();
                    Button::new((id, ix))
                        .ghost()
                        .xsmall()
                        .selected(ix == current)
                        .toggled(ix == current)
                        .label(format!("{label} {n}"))
                        .on_click(move |_, window, cx| on(&ix, window, cx))
                }),
        )
}

/// A button that opens a menu of `options` with a check by the current one.
pub fn menu(
    id: &'static str,
    icon: Lucide,
    label: String,
    options: Vec<&'static str>,
    current: usize,
    on: impl Fn(&usize, &mut Window, &mut App) + 'static,
) -> AnyElement {
    let on: OnPick = Rc::new(on);
    Button::new(id)
        .outline()
        .small()
        .icon(icon)
        .label(label)
        .dropdown_menu_with_anchor(Anchor::TopRight, move |mut menu, _, _| {
            for (ix, option) in options.iter().enumerate() {
                let on = on.clone();
                menu = menu.item(
                    PopupMenuItem::new(*option)
                        .checked(ix == current)
                        .on_click(move |_, window, cx| on(&ix, window, cx)),
                );
            }
            menu
        })
        .into_any_element()
}

/// What a status pill leads with.
pub enum Mark {
    Icon(Lucide),
    Spinner,
}

/// A status: icon and words on a tint of `color`, so it never rests on the
/// colour alone.
pub fn pill(mark: Mark, text: impl Into<SharedString>, color: Hsla) -> Div {
    h_flex()
        .flex_none()
        .max_w_full()
        .h(px(22.))
        .px_2()
        .gap_1()
        .rounded_full()
        .border_1()
        .border_color(color.opacity(0.3))
        .bg(color.opacity(0.1))
        .text_xs()
        .text_color(color)
        .child(match mark {
            Mark::Icon(icon) => Icon::new(icon).xsmall().into_any_element(),
            Mark::Spinner => Spinner::new().xsmall().color(color).into_any_element(),
        })
        .child(div().min_w_0().truncate().child(text.into()))
}

/// A neutral label next to a name: "Remote", "Sign-in", "Runtime".
pub fn tag(icon: Option<Lucide>, text: impl Into<SharedString>, cx: &App) -> Div {
    h_flex()
        .flex_none()
        .h(px(18.))
        .px_1p5()
        .gap_1()
        .rounded(cx.theme().radius)
        .bg(cx.theme().secondary)
        .text_xs()
        .text_color(cx.theme().muted_foreground)
        .when_some(icon, |el, icon| el.child(Icon::new(icon).xsmall()))
        .child(text.into())
}

/// A logo on a square tile, so logos of any shape line up.
pub fn tile(logo: AnyElement, cx: &App) -> Div {
    div()
        .size(px(TILE))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .rounded(cx.theme().radius_lg)
        .border_1()
        .border_color(cx.theme().border)
        .bg(cx.theme().secondary)
        .child(logo)
}

/// A line of the list, the header's or a row's: `lead` (a checkbox or a gap)
/// and the tile, then name, status and actions in their columns. Where the
/// name would be narrower than `NAME_MIN`, status and then actions wrap under
/// it, actions still on the right, so a narrow page shows every column.
pub fn columns(
    lead: Option<AnyElement>,
    tile: impl IntoElement,
    name: impl IntoElement,
    status: impl IntoElement,
    actions: impl IntoElement,
) -> Div {
    h_flex()
        .gap_3()
        .items_start()
        .when_some(lead, |el, lead| {
            el.child(div().w(px(16.)).flex_none().child(lead))
        })
        .child(tile)
        .child(
            h_flex()
                .flex_1()
                .min_w_0()
                .flex_wrap()
                .items_start()
                .gap_x_3()
                .gap_y_2()
                .child(
                    div()
                        .flex_grow_1()
                        .flex_basis(px(NAME_MIN))
                        .min_w_0()
                        .child(name),
                )
                .child(
                    v_flex()
                        .w(px(STATUS_W))
                        .flex_none()
                        .max_w_full()
                        .items_start()
                        .child(status),
                )
                .child(
                    h_flex()
                        .w(px(ACTIONS_W))
                        .flex_none()
                        .max_w_full()
                        .ml_auto()
                        .justify_end()
                        .child(actions),
                ),
        )
}

/// The list's column titles: `lead` (a checkbox or a gap) over the
/// checkboxes, then name, status and actions over their columns. It keeps to
/// one line: a column that wrapped under the name has nothing to title.
pub fn header(lead: Option<AnyElement>, name: &'static str, cx: &App) -> impl IntoElement + use<> {
    columns(
        lead,
        div().w(px(TILE)).flex_none(),
        name,
        "Status",
        "Actions",
    )
    .h(px(32.))
    .px_4()
    .py_2()
    .overflow_hidden()
    .border_b_1()
    .border_color(cx.theme().border)
    .bg(cx.theme().secondary)
    .text_xs()
    .font_medium()
    .text_color(cx.theme().muted_foreground)
}

type OnPaging = Rc<dyn Fn(&Paging, &mut Window, &mut App)>;
type OnPageSize = Rc<dyn Fn(&PageSize, &mut Window, &mut App)>;

/// The Show menu: "Show 8", which opens `choices` ("Rows per page") with a check by `current`,
/// from its `anchor` corner. A size picked is kept as list `list`'s, for the next view opened and
/// the next launch ([`saved_size`]), and handed to `on`.
pub fn page_size_menu(
    list: &'static str,
    current: PageSize,
    choices: &'static [PageSize],
    anchor: Anchor,
    on: impl Fn(&PageSize, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    let on: OnPageSize = Rc::new(on);
    Button::new(SharedString::from(format!("{list}-page-size")))
        .debug_selector(move || format!("{list}-page-size"))
        .ghost()
        .xsmall()
        .label(format!("Show {}", current.label()))
        .dropdown_caret(true)
        .tooltip("Rows per Page")
        .dropdown_menu_with_anchor(anchor, move |mut menu, _, _| {
            menu = menu.label("Rows per page");
            for size in choices.iter().copied() {
                let on = on.clone();
                menu = menu.item(
                    PopupMenuItem::new(size.label())
                        .checked(size == current)
                        .on_click(move |_, window, cx| {
                            save_size(list, size, cx);
                            on(&size, window, cx);
                        }),
                );
            }
            menu
        })
}

/// Under the list: "9–16 of 24 tools", the Show menu (how many rows a page holds) and the page
/// buttons. `list` names its elements and keeps the size picked for the next page opened and the
/// next launch ([`Paging::new`]); `paging` is what [`page_of`] gave. `on` gets the page turned
/// to, or the new size on the page holding row `selected` (the list's selected row, by its place
/// in the whole list) when there is one, else the row that was first on the page. Too narrow for
/// one line, the page buttons wrap under the count, on the right.
pub fn pager(
    list: &'static str,
    paging: Paging,
    total: usize,
    noun: &str,
    selected: Option<usize>,
    on: impl Fn(&Paging, &mut Window, &mut App) + 'static,
    cx: &App,
) -> Div {
    let on: OnPaging = Rc::new(on);
    let per_page = paging.size.rows(total);
    let first = (paging.page * per_page + 1).min(total);
    let last = ((paging.page + 1) * per_page).min(total);
    let pages = paging.size.pages(total);
    let resize = on.clone();
    // Under the list, near the window's foot: the menu opens upwards.
    let size_menu = page_size_menu(
        list,
        paging.size,
        &PageSize::CHOICES,
        Anchor::BottomLeft,
        move |size, window, cx| resize(&paging.resized(*size, selected), window, cx),
    );
    h_flex()
        .px_4()
        .py_1()
        .min_h(px(40.))
        .flex_wrap()
        .gap_x_4()
        .gap_y_1()
        .justify_between()
        .bg(cx.theme().secondary)
        .text_xs()
        .text_color(cx.theme().muted_foreground)
        .child(
            h_flex()
                .debug_selector(move || format!("{list}-count"))
                .flex_none()
                .gap_2()
                .child(label(
                    SharedString::from(format!("{list}-count")),
                    format!("{first}–{last} of {total} {noun}"),
                ))
                // Fewer rows than the smallest page: the size changes nothing.
                .when(total > PageSize::CHOICES[0].rows(total), |el| {
                    el.child(size_menu)
                }),
        )
        .when(pages > 1, |el| {
            el.child(
                div()
                    .debug_selector(move || format!("{list}-pages"))
                    .flex_none()
                    .ml_auto()
                    .child(
                        Pagination::new(SharedString::from(format!("{list}-pages")))
                            .xsmall()
                            .current_page(paging.page + 1)
                            .total_pages(pages)
                            .on_click(move |p, window, cx| {
                                on(
                                    &Paging {
                                        page: p - 1,
                                        ..paging
                                    },
                                    window,
                                    cx,
                                )
                            }),
                    ),
            )
        })
}

/// Nothing matches: says so and offers to clear what hides the rows.
pub fn empty(title: String, clear: Button, cx: &App) -> impl IntoElement + use<> {
    let muted = cx.theme().muted_foreground;
    v_flex()
        .py_12()
        .gap_2()
        .items_center()
        .child(Icon::new(Lucide::SearchX).large().text_color(muted))
        .child(div().text_sm().font_semibold().child(title))
        .child(
            div()
                .text_xs()
                .text_color(muted)
                .child("Try another search, or clear the filters."),
        )
        .child(div().pt_1().child(clear))
}

/// Text that assistive technology reads as text: GPUI exposes no plain text to it, so a count or
/// a status that matters on its own is shown through this, under its element `id`.
pub fn label(id: impl Into<ElementId>, text: impl Into<SharedString>) -> Stateful<Div> {
    let text = text.into();
    div()
        .id(id)
        .role(Role::Label)
        .aria_label(text.clone())
        .child(text)
}

/// A row of a catalog list as assistive technology sees it: an item named `name`, so a screen
/// reader (and a test) can tell the rows apart rather than hearing only their buttons.
pub fn list_item(
    row: Div,
    id: impl Into<ElementId>,
    name: impl Into<SharedString>,
) -> Stateful<Div> {
    row.id(id).role(Role::ListItem).aria_label(name)
}

/// A thin upright rule between groups of buttons.
pub fn divider(cx: &App) -> impl IntoElement + use<> {
    div()
        .flex_none()
        .w(px(1.))
        .h(px(14.))
        .mx_1()
        .bg(cx.theme().border)
}

#[cfg(test)]
mod tests {
    use gpui_kit::{
        Bounds, Context, InteractiveElement as _, IntoElement, ParentElement as _, Pixels, Render,
        Styled as _, TestAppContext, VisualTestContext, Window, div, px, size,
    };

    use super::{
        NAME_MIN, PageSize, Paging, TILE, columns, keep_page_sizes, page_of, pager, save_size,
        saved_size, tilde_in,
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
}
