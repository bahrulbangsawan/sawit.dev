//! Exporting rows to CSV or Excel: the page shown, or every row the query matches (streamed
//! over a connection of its own, with its progress), and the toolbar's Export menu.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64};
use std::time::Duration;

use futures::channel::oneshot;
use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::*;
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_kit::component::*;
use gpui_kit::*;
use sawit_core::db::{self, ExportFormat, ExportSummary, RowCount};

use super::{TableView, plural};
use crate::db::actions;
use crate::rt;

/// An export of every matching row, writing in the background.
pub(super) struct Exporting {
    pub(super) format: ExportFormat,
    pub(super) rows: Arc<AtomicU64>,
    pub(super) stop: Arc<AtomicBool>,
}

/// How often the export's row count refreshes.
const PROGRESS_TICK: Duration = Duration::from_millis(250);

/// Exports.
impl TableView {
    /// Asks where to save, then writes this page or every matching row.
    fn export(
        &mut self,
        format: ExportFormat,
        all: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let name = format!("{}.{}", self.table, format.extension());
        let picked = cx.prompt_for_new_path(&actions::save_dir(cx), Some(&name));
        cx.spawn_in(window, async move |this, window| {
            let Ok(Ok(Some(path))) = picked.await else {
                return;
            };
            let _ = this.update_in(window, |this, window, cx| {
                if all {
                    this.export_all(format, path, window, cx);
                } else {
                    this.export_page(format, path, window, cx);
                }
            });
        })
        .detach();
    }

    fn column_types(&self, cx: &App) -> Vec<db::ColumnInfo> {
        self.db
            .read(cx)
            .relation(&self.profile, &self.schema, &self.table)
            .map(|r| r.columns.clone())
            .unwrap_or_default()
    }

    fn export_page(
        &mut self,
        format: ExportFormat,
        path: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(set) = self
            .grid
            .as_ref()
            .map(|g| g.read(cx).delegate().set.clone())
        else {
            return;
        };
        let (types, sheet) = (self.column_types(cx), self.table.clone());
        let target = path.clone();
        let rx = rt::spawn_blocking(cx, move || {
            db::export_result_set(&set, format, &sheet, &types, &target)
        });
        cx.spawn_in(window, async move |this, window| {
            let result = rx.await;
            let _ = this.update_in(window, |this, _, cx| this.exported(&path, result, cx));
        })
        .detach();
    }

    /// Streams over a connection of its own, so browsing stays responsive.
    fn export_all(
        &mut self,
        format: ExportFormat,
        path: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let rows = Arc::new(AtomicU64::new(0));
        let stop = Arc::new(AtomicBool::new(false));
        let Some(rx) = actions::export_relation(
            &self.db,
            &self.profile,
            self.schema.clone(),
            self.table.clone(),
            self.order_by.clone().unwrap_or_default(),
            self.query.clone(),
            format,
            path.clone(),
            rows.clone(),
            stop.clone(),
            cx,
        ) else {
            return;
        };
        self.exporting = Some(Exporting { format, rows, stop });
        self.notice = None;
        cx.notify();
        cx.spawn_in(window, async move |this, window| {
            let mut rx = rx;
            // Redraws the row count until the export ends.
            let result = loop {
                let tick = window.background_executor().timer(PROGRESS_TICK);
                match futures::future::select(rx, Box::pin(tick)).await {
                    futures::future::Either::Left((result, _)) => break result,
                    futures::future::Either::Right((_, pending)) => {
                        rx = pending;
                        if this.update(window, |_, cx| cx.notify()).is_err() {
                            return;
                        }
                    }
                }
            };
            let _ = this.update_in(window, |this, _, cx| {
                this.exporting = None;
                this.exported(&path, result, cx);
            });
        })
        .detach();
    }

    fn exported(
        &mut self,
        path: &std::path::Path,
        result: Result<sawit_core::Result<ExportSummary>, oneshot::Canceled>,
        cx: &mut Context<Self>,
    ) {
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        self.notice = Some(match result {
            Ok(Ok(ExportSummary {
                rows,
                truncated: false,
            })) => (false, format!("Saved {} to {name}.", plural(rows, "row"))),
            Ok(Ok(ExportSummary {
                rows,
                truncated: true,
            })) => (
                false,
                format!(
                    "Saved the first {rows} rows to {name}: Excel holds no more. Export as CSV for every row."
                ),
            ),
            Ok(Err(e)) => (true, format!("Unable to export {name}. {e}")),
            Err(_) => (true, "The export stopped unexpectedly. Try again.".into()),
        });
        cx.notify();
    }

    pub(super) fn export_menu(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let view = cx.entity().downgrade();
        let page_rows = self.pager.page_rows as u64;
        let filtered = self.query.is_filtered();
        let all_label = match self.pager.total {
            Some(RowCount { rows, exact: true }) if filtered => format!("All {rows} Matching Rows"),
            Some(RowCount { rows, exact: true }) => format!("All {rows} Rows"),
            _ if filtered => "All Matching Rows".to_string(),
            _ => "All Rows".to_string(),
        };
        let has_rows = page_rows > 0;
        Button::new("table-export")
            .ghost()
            .xsmall()
            .icon(Lucide::Download)
            .label("Export")
            .tooltip("Save Rows as CSV or Excel")
            .disabled(!has_rows || self.exporting.is_some())
            .dropdown_menu(move |mut menu, _, _| {
                let item = |label: String, icon: Lucide, format: ExportFormat, all: bool| {
                    let view = view.clone();
                    PopupMenuItem::new(label)
                        .icon(icon)
                        .on_click(move |_, window, cx| {
                            let _ =
                                view.update(cx, |this, cx| this.export(format, all, window, cx));
                        })
                };
                menu = menu.label(format!("This Page ({})", plural(page_rows, "row")));
                for format in [ExportFormat::Csv, ExportFormat::Xlsx] {
                    menu = menu.item(item(
                        format!("{}…", format.label()),
                        icon_for(format),
                        format,
                        false,
                    ));
                }
                menu = menu.separator().label(all_label.clone());
                for format in [ExportFormat::Csv, ExportFormat::Xlsx] {
                    menu = menu.item(item(
                        format!("{}…", format.label()),
                        icon_for(format),
                        format,
                        true,
                    ));
                }
                menu
            })
    }
}

fn icon_for(format: ExportFormat) -> Lucide {
    match format {
        ExportFormat::Csv => Lucide::FileText,
        ExportFormat::Xlsx => Lucide::FileSpreadsheet,
    }
}
