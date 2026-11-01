//! Which page is shown and what is known of the total: the offset, the last page, whether there
//! is a page before or after, and the toolbar's label.

use sawit_core::db::RowCount;

use super::PAGE_SIZE;

/// Which page is shown, how many rows a page holds, and what is known about the total.
#[derive(Debug, Clone)]
pub struct Pager {
    pub page: u64,
    /// Rows per page (the most a page ever shows).
    pub size: u64,
    /// `None` while the count is still running.
    pub total: Option<RowCount>,
    /// Rows on the current page.
    pub page_rows: usize,
}

impl Default for Pager {
    fn default() -> Self {
        Self {
            page: 0,
            size: PAGE_SIZE,
            total: None,
            page_rows: 0,
        }
    }
}

impl Pager {
    pub fn offset(&self) -> u64 {
        self.page * self.size
    }

    /// Index of the last page, when the total is exact.
    pub fn last_page(&self) -> Option<u64> {
        match self.total {
            Some(RowCount { rows, exact: true }) => Some(rows.saturating_sub(1) / self.size),
            _ => None,
        }
    }

    pub fn has_prev(&self) -> bool {
        self.page > 0
    }

    pub fn has_next(&self) -> bool {
        match self.last_page() {
            Some(last) => self.page < last,
            None => self.page_rows as u64 == self.size,
        }
    }

    /// The page holding the row first on the page shown, at `size` rows a page.
    pub(super) fn resized(&self, size: u64) -> Self {
        let size = size.max(1);
        Self {
            page: self.offset() / size,
            size,
            ..self.clone()
        }
    }

    pub fn label(&self) -> String {
        if self.page_rows == 0 && self.page == 0 {
            return "0 rows".into();
        }
        let range = format!(
            "{}–{}",
            self.offset() + 1,
            self.offset() + self.page_rows as u64
        );
        match self.total {
            Some(RowCount { rows, exact: true }) => format!("{range} of {rows}"),
            Some(RowCount { rows, exact: false }) => format!("{range} of ±{rows}"),
            None => range,
        }
    }
}
