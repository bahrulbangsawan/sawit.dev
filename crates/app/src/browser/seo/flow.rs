//! The browser's side of the SEO scan: the page's rendered HTML asked for (collecting only), the
//! scan run in Rust off the main thread, the report handed to the panel, and the share image and favicon fetched for the previews.

use std::sync::Arc;

use futures::channel::oneshot;
use gpui_kit::*;
use sawit_core::web::favicon;
use sawit_core::web::seo::{self, Report, ScanOptions, ScanType};

use super::super::{BrowserView, OpResult, Panel};
use crate::rt;

/// The rendered page, as HTML: the DOM after the page's scripts ran.
const RENDERED_HTML_JS: &str = "document.documentElement.outerHTML";
/// The widest the share image is decoded for the previews.
const PICTURE_PX: u32 = 640;

/// What a scan is asked for.
#[derive(Clone, Debug, Default)]
pub struct SeoAsk {
    pub checks: Vec<String>,
    pub scan_type: ScanType,
    pub external: bool,
}

impl BrowserView {
    /// The panel's Rescan, or the panel opened.
    pub(in crate::browser) fn seo_rescan(&mut self, cx: &mut Context<Self>) {
        let ask = self.seo_ask.clone();
        self.seo_scan(ask, None, cx);
    }

    /// A page finished loading while the panel is open: scan it.
    pub(in crate::browser) fn seo_after_load(&mut self, cx: &mut Context<Self>) {
        if self.panel == Some(Panel::Seo) {
            self.seo_rescan(cx);
        }
    }

    /// Scans the page on show; the panel shows it, and `reply`, if any, gets the report.
    pub(in crate::browser) fn seo_scan(
        &mut self,
        ask: SeoAsk,
        reply: Option<oneshot::Sender<OpResult>>,
        cx: &mut Context<Self>,
    ) {
        let url = self.current_url();
        if !(url.starts_with("http://") || url.starts_with("https://")) {
            self.seo_panel.update(cx, |p, cx| p.set_idle(cx));
            if let Some(reply) = reply {
                let _ = reply.send(Err(format!(
                    "The tab has no web page to scan ({url}). Open one with browser_navigate, or pass url."
                )));
            }
            return;
        }
        let host = super::host_of(&url);
        self.seo_panel.update(cx, |p, cx| p.set_loading(host, cx));
        self.seo_generation += 1;
        let generation = self.seo_generation;
        let html = self.rendered_html();
        cx.spawn(async move |this, cx| {
            let rendered_html = html.await.ok().flatten();
            let options = ScanOptions {
                scan_type: ask.scan_type,
                checks: ask.checks,
                external: ask.external,
                rendered_html,
            };
            let Ok(task) = this.update(cx, |_, cx| {
                let url = url.clone();
                rt::spawn(cx, async move { seo::scan(&url, options).await })
            }) else {
                return;
            };
            let Ok(report) = task.await else {
                if let Some(reply) = reply {
                    let _ = reply.send(Err("The scan stopped before it finished.".into()));
                }
                return;
            };
            let report = Arc::new(report);
            let _ = this.update(cx, |this, cx| {
                if generation == this.seo_generation {
                    this.seo_panel
                        .update(cx, |p, cx| p.set_report(report.clone(), cx));
                    this.seo_pictures(&report, generation, cx);
                }
            });
            if let Some(reply) = reply {
                let _ = reply.send(serde_json::to_value(&*report).map_err(|e| e.to_string()));
            }
        })
        .detach();
    }

    /// The page's rendered HTML, or `None` when the page can't be asked (no view, a script
    /// error): the scan then reads the server's HTML alone.
    fn rendered_html(&self) -> oneshot::Receiver<Option<String>> {
        let (tx, rx) = oneshot::channel();
        match &self.engine {
            Some(engine) => engine.eval_text(RENDERED_HTML_JS, move |result| {
                let _ = tx.send(result.ok().flatten());
            }),
            None => {
                let _ = tx.send(None);
            }
        }
        rx
    }

    /// Fetches the share image and the favicon for the previews.
    fn seo_pictures(&mut self, report: &Report, generation: u64, cx: &mut Context<Self>) {
        let image = report
            .facts
            .og_image
            .clone()
            .or_else(|| report.facts.twitter_image.clone());
        let page = report.final_url.clone();
        let task = rt::spawn(cx, async move {
            let picture = match image {
                Some(url) => favicon::load_picture(&url, PICTURE_PX).await,
                None => None,
            };
            let icon = favicon::favicon(&page, None).await;
            (picture, icon)
        });
        cx.spawn(async move |this, cx| {
            let Ok((picture, icon)) = task.await else {
                return;
            };
            let _ = this.update(cx, |this, cx| {
                if generation != this.seo_generation {
                    return;
                }
                let picture = picture
                    .as_ref()
                    .and_then(crate::browser::favicon::render_image);
                let icon = icon
                    .as_deref()
                    .and_then(crate::browser::favicon::render_image);
                this.seo_panel
                    .update(cx, |p, cx| p.set_pictures(picture, icon, cx));
            });
        })
        .detach();
    }
}
