//! Tests of the pager, the page size kept for every table, and the cells' previews.

// Not a glob: `gpui_kit::*` exports a `test` macro that shadows #[test].
use super::grid::{CELL_PREVIEW_CHARS, cell_preview};
use super::{PAGE_SIZE, PAGE_SIZES, Pager, saved_page_size};

#[test]
fn cell_preview_is_one_short_line() {
    assert_eq!(cell_preview("a\nb"), "a b");
    let long = "x".repeat(10_000);
    let shown = cell_preview(&long);
    assert_eq!(shown.chars().count(), CELL_PREVIEW_CHARS + 1);
    assert!(shown.ends_with('…'));
    assert_eq!(
        cell_preview(&"y".repeat(CELL_PREVIEW_CHARS))
            .chars()
            .count(),
        CELL_PREVIEW_CHARS
    );
}
use sawit_core::db::RowCount;

fn exact(rows: u64) -> Option<RowCount> {
    Some(RowCount { rows, exact: true })
}

#[test]
fn page_size_is_fifty() {
    assert_eq!(PAGE_SIZE, 50);
    assert_eq!(Pager::default().size, 50);
}

/// A page holds the rows the Show menu says; a new size shows the page with the row that was
/// first on the page before.
#[test]
fn a_new_size_keeps_the_first_row_shown() {
    let p = Pager {
        page: 3,
        total: exact(1234),
        page_rows: 50,
        ..Default::default()
    };
    assert_eq!(p.offset(), 150);
    let bigger = p.resized(100);
    assert_eq!((bigger.page, bigger.offset()), (1, 100));
    assert_eq!(bigger.last_page(), Some(12));
    assert_eq!(
        bigger.label(),
        "101–150 of 1234",
        "rows shown until the page loads"
    );
    let all_but = p.resized(1000);
    assert_eq!((all_but.page, all_but.last_page()), (0, Some(1)));
    // Back to 50 from page 1 of 100 (rows 101–200): row 101 is on page 2.
    assert_eq!(bigger.resized(50).page, 2);
    // Unknown total: a full page means "maybe more".
    let p = Pager {
        size: 250,
        page_rows: 250,
        ..Default::default()
    };
    assert!(p.has_next());
    assert_eq!(p.label(), "1–250");
}

/// The Show menu offers sizes, never every row, and a size picked for one table is every
/// table's; any other size kept (an older menu's, a file edited by hand) is 50.
#[gpui_kit::test]
fn tables_start_at_the_size_last_picked(cx: &mut gpui_kit::TestAppContext) {
    use crate::catalog::{PageSize, keep_page_sizes};
    assert!(!PAGE_SIZES.contains(&PageSize::All));
    let file = crate::testing::scratch_dir("table-page-size").join("page-sizes.json");
    for (saved, size) in [
        (r#"{"table": {"rows": 250}}"#, 250),
        (r#"{"table": {"rows": 7}}"#, 50),
        (r#"{"table": "all"}"#, 50),
        (r#"{"tools": {"rows": 16}}"#, 50),
    ] {
        std::fs::write(&file, saved).unwrap();
        cx.update(|cx| {
            keep_page_sizes(file.clone(), cx);
            assert_eq!(saved_page_size(cx), size, "{saved}");
        });
    }
}

#[test]
fn exact_totals_bound_navigation() {
    let mut p = Pager {
        total: exact(123),
        page_rows: 50,
        ..Default::default()
    };
    assert_eq!(p.offset(), 0);
    assert!(!p.has_prev());
    assert!(p.has_next());
    assert_eq!(p.last_page(), Some(2));
    assert_eq!(p.label(), "1–50 of 123");
    p.page = 2;
    p.page_rows = 23;
    assert_eq!(p.offset(), 100);
    assert!(!p.has_next());
    assert!(p.has_prev());
    assert_eq!(p.label(), "101–123 of 123");
}

#[test]
fn empty_and_unknown_totals() {
    let p = Pager {
        total: exact(0),
        ..Default::default()
    };
    assert_eq!(p.last_page(), Some(0));
    assert!(!p.has_next());
    assert_eq!(p.label(), "0 rows");
    // Count still running or only estimated: a full page means "maybe more".
    let mut p = Pager {
        page_rows: 50,
        ..Default::default()
    };
    assert!(p.has_next());
    assert_eq!(p.last_page(), None);
    assert_eq!(p.label(), "1–50");
    p.total = Some(RowCount {
        rows: 2_000_000,
        exact: false,
    });
    assert_eq!(p.label(), "1–50 of ±2000000");
    p.page_rows = 7;
    assert!(!p.has_next());
}
