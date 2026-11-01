//! Paging a catalog list: how many rows a page holds, which page is shown, the rows on it, and
//! the size each list was left at, kept across launches.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use gpui_kit::{App, AppContext as _, Global};
use serde::{Deserialize, Serialize};

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
    pub(super) fn rows(self, total: usize) -> usize {
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
    pub(super) fn label(self) -> String {
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
pub(super) fn save_size(list: &str, size: PageSize, cx: &mut App) {
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
