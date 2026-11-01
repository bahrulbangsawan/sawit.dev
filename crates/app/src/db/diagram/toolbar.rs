//! The toolbar: table and relation counts, the search with its matches, the DBML view, and
//! copying or saving the DBML and the SVG.

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::*;
use gpui_kit::component::input::Input;
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use sawit_core::db::diagram;
use sawit_core::mermaid;

use super::DiagramView;
use crate::db::actions;
use crate::rt;

impl DiagramView {
    // Exports.

    fn copy_dbml(&mut self, cx: &mut Context<Self>) {
        cx.write_to_clipboard(ClipboardItem::new_string(self.dbml.clone()));
        self.notice = Some("DBML copied.".into());
        cx.notify();
    }

    fn export_svg(&mut self, cx: &mut Context<Self>) {
        let Some(schemas) = self.db.read(cx).schemas.get(&self.profile).cloned() else {
            return;
        };
        let source = diagram::to_mermaid(&schemas, Some(&self.schema));
        let palette = crate::editor::palette(cx);
        self.exporting = true;
        cx.notify();
        let rx = rt::spawn_blocking(cx, move || mermaid::render(&source, &palette));
        cx.spawn(async move |this, cx| {
            let result = rx.await;
            let _ = this.update(cx, |this, cx| {
                this.exporting = false;
                match result {
                    Ok(Ok(d)) => this.save("svg", d.svg, cx),
                    Ok(Err(e)) => this.notice = Some(format!("Unable to draw the SVG. {e}")),
                    Err(_) => this.notice = Some("Unable to draw the SVG. Try again.".into()),
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Asks where to save `contents` (default name `<schema>.<ext>`).
    fn save(&mut self, ext: &str, contents: String, cx: &mut Context<Self>) {
        let name = format!("{}.{ext}", self.schema);
        let dir = actions::save_dir(cx);
        let picked = cx.prompt_for_new_path(&dir, Some(&name));
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(path))) = picked.await else {
                return;
            };
            let target = path.clone();
            let Ok(rx) = this.update(cx, |_, cx| {
                rt::spawn(cx, async move { std::fs::write(&target, contents) })
            }) else {
                return;
            };
            let result = rx.await;
            let _ = this.update(cx, |this, cx| {
                let name = path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                this.notice = Some(match result {
                    Ok(Ok(())) => format!("Saved {name}."),
                    Ok(Err(e)) => format!("Unable to save {name}. {e}"),
                    Err(_) => "Unable to save. Try again.".into(),
                });
                cx.notify();
            });
        })
        .detach();
    }

    pub(super) fn render_toolbar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let muted = theme.muted_foreground;
        let tables = self.erd.tables.len();
        let status = if self.exporting {
            "Drawing SVG…".to_string()
        } else if let Some(n) = self.notice.clone() {
            n
        } else {
            let refs = self.erd.edges.len();
            format!(
                "{tables} {} · {refs} {}",
                if tables == 1 { "table" } else { "tables" },
                if refs == 1 { "relation" } else { "relations" }
            )
        };
        let query_empty = self.search.read(cx).value().trim().is_empty();
        let hits = self.hits.len();
        h_flex()
            .px_3()
            .py_1()
            .gap_2()
            .text_xs()
            .text_color(muted)
            .border_b_1()
            .border_color(theme.border)
            .child(
                div()
                    .text_color(theme.foreground)
                    .font_semibold()
                    .child(self.title.clone()),
            )
            .child(div().truncate().child(status))
            .child(div().flex_1())
            .when(!self.show_dbml, |el| {
                el.child(
                    div().w(px(220.)).child(
                        Input::new(&self.search)
                            .xsmall()
                            .prefix(Icon::new(Lucide::Search).xsmall().text_color(muted))
                            .cleanable(true),
                    ),
                )
                .when(!query_empty, |el| {
                    el.child(
                        div()
                            .min_w(px(44.))
                            .text_center()
                            .when(hits == 0, |el| el.text_color(theme.danger))
                            .child(if hits == 0 {
                                "No match".to_string()
                            } else {
                                format!("{}/{hits}", self.hit + 1)
                            }),
                    )
                    .child(
                        Button::new("erd-hit-prev")
                            .ghost()
                            .xsmall()
                            .icon(Lucide::ChevronUp)
                            .tooltip("Previous Match (⇧↩)")
                            .disabled(hits < 2)
                            .on_click(cx.listener(|this, _, _, cx| this.step_hit(-1, cx))),
                    )
                    .child(
                        Button::new("erd-hit-next")
                            .ghost()
                            .xsmall()
                            .icon(Lucide::ChevronDown)
                            .tooltip("Next Match (↩)")
                            .disabled(hits < 2)
                            .on_click(cx.listener(|this, _, _, cx| this.step_hit(1, cx))),
                    )
                })
            })
            .child(
                Button::new("diagram-dbml")
                    .ghost()
                    .xsmall()
                    .label("DBML")
                    .selected(self.show_dbml)
                    .tooltip("Show Schema as DBML")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.show_dbml = !this.show_dbml;
                        cx.notify();
                    })),
            )
            .child(
                Button::new("diagram-copy")
                    .ghost()
                    .xsmall()
                    .icon(Lucide::Copy)
                    .tooltip("Copy DBML")
                    .on_click(cx.listener(|this, _, _, cx| this.copy_dbml(cx))),
            )
            .child(
                Button::new("diagram-save-dbml")
                    .ghost()
                    .xsmall()
                    .icon(Lucide::Download)
                    .label(".dbml")
                    .tooltip("Save DBML…")
                    .on_click(cx.listener(|this, _, _, cx| {
                        let dbml = this.dbml.clone();
                        this.save("dbml", dbml, cx)
                    })),
            )
            .child(
                Button::new("diagram-save-svg")
                    .ghost()
                    .xsmall()
                    .icon(Lucide::Download)
                    .label(".svg")
                    .tooltip("Save Diagram as SVG…")
                    .disabled(tables == 0 || self.exporting)
                    .on_click(cx.listener(|this, _, _, cx| this.export_svg(cx))),
            )
            .child(
                Button::new("diagram-reload")
                    .ghost()
                    .xsmall()
                    .icon(Lucide::RefreshCw)
                    .tooltip("Reload Schema")
                    .on_click(cx.listener(|this, _, _, cx| this.reload(cx))),
            )
    }
}
