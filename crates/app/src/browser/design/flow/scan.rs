//! The page's scan: the page's own read, the sheets it couldn't read fetched by Sawit and fed
//! back through `rules`, its manifest's icons, then the tokens; one scan per page, shared.

use std::rc::Rc;
use std::time::Duration;

use futures::FutureExt as _;
use futures::StreamExt as _;
use futures::channel::oneshot;
use gpui_kit::{AsyncApp, Context, Task, WeakEntity};
use sawit_core::annotations::page_key;
use sawit_core::site_design::assets::{fetch_text, may_fetch_for, sheet_for_page};
use sawit_core::site_design::scan::{AssetKind, AssetRef, Scan, SheetRules};
use sawit_core::site_design::tokens::derive;
use serde_json::{Value, json};

use super::{NOTHING, Ready, ScanState, cancelled, extractable, gone, page_answer, parse};
use crate::browser::{BrowserView, OpResult};
use crate::rt;

/// All cross-origin sheets and the manifest, together.
/// From the start of the scan, so `Design` answers within the 30 s control timeout.
const FETCH_BUDGET: Duration = Duration::from_secs(20);

const FETCHES_AT_ONCE: usize = 8;

impl BrowserView {
    /// The page's scan with fetched sheets, manifest icons and tokens; cached per page.
    pub(crate) fn scan_design(
        &mut self,
        cx: &mut Context<Self>,
    ) -> Task<Result<Rc<Ready>, String>> {
        let url = self.current_url();
        if !extractable(&url) {
            self.design.state = ScanState::Failed(NOTHING.into());
            self.sync_design(cx);
            return Task::ready(Err(NOTHING.into()));
        }
        let key = page_key(&url).to_string();
        if self.design.key != key {
            self.design.reset();
            self.design.key = key;
        }
        let job = match (&self.design.state, &self.design.job) {
            (ScanState::Ready(ready), _) => return Task::ready(Ok(ready.clone())),
            (ScanState::Scanning, Some(job)) => job.clone(),
            _ => {
                let job = self.start_scan(url, cx).shared();
                self.design.job = Some(job.clone());
                job
            }
        };
        cx.spawn(async move |_, _| job.await)
    }

    fn start_scan(
        &mut self,
        url: String,
        cx: &mut Context<Self>,
    ) -> Task<Result<Rc<Ready>, String>> {
        let generation = self.design.generation;
        let fetch_deadline = std::time::Instant::now() + FETCH_BUDGET;
        let scan = self.page_op(json!({ "op": "scan" }));
        self.design.state = ScanState::Scanning;
        self.sync_design(cx);
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = run_scan(&this, scan, url, fetch_deadline, cx).await;
            let _ = this.update(cx, |this, cx| {
                if this.design.generation == generation {
                    this.design.state = match &result {
                        Ok(ready) => ScanState::Ready(ready.clone()),
                        Err(e) => ScanState::Failed(e.clone().into()),
                    };
                    this.sync_design(cx);
                    cx.notify();
                }
            });
            result
        })
    }
}

async fn run_scan(
    this: &WeakEntity<BrowserView>,
    scan: oneshot::Receiver<OpResult>,
    url: String,
    fetch_deadline: std::time::Instant,
    cx: &mut AsyncApp,
) -> Result<Rc<Ready>, String> {
    let mut scan: Scan = parse("scan", serde_json::from_value(page_answer(scan, cx).await?))?;
    if scan.colors.is_empty() && scan.texts.is_empty() {
        return Err(NOTHING.into());
    }
    let referer = if scan.url.is_empty() {
        url.clone()
    } else {
        scan.url.clone()
    };
    let extras = fetch_extras(
        scan.blocked_sheets.clone(),
        scan.manifest.clone(),
        referer,
        url,
        fetch_deadline,
    );
    let extras = this
        .update(cx, |_, cx| rt::spawn(cx, extras))
        .map_err(gone)?
        .await
        .map_err(cancelled)?;
    if !extras.sheets.is_empty() {
        // `rules` replaces the page's fetched sheets: all of them in one call.
        let sheets: Vec<Value> = extras
            .sheets
            .iter()
            .map(|(href, text)| json!({ "href": href, "text": text }))
            .collect();
        let rules = this
            .update(cx, |this, _| {
                this.page_op(json!({ "op": "rules", "sheets": sheets }))
            })
            .map_err(gone)?;
        let rules: SheetRules = parse(
            "rules",
            serde_json::from_value(page_answer(rules, cx).await?),
        )?;
        scan.merge_rules(rules);
    }
    add_icons(&mut scan.assets, extras.icons);
    let date = sawit_core::date::local_date();
    let (scan, tokens) = this
        .update(cx, |_, cx| {
            rt::spawn_blocking(cx, move || {
                let tokens = derive(&scan, &date);
                (scan, tokens)
            })
        })
        .map_err(gone)?
        .await
        .map_err(cancelled)?;
    Ok(Rc::new(Ready { scan, tokens }))
}

struct Extras {
    /// (href, text) in the page's order.
    sheets: Vec<(String, String)>,
    icons: Vec<AssetRef>,
}

fn is_http(url: &str) -> bool {
    let l = url.to_ascii_lowercase();
    l.starts_with("http://") || l.starts_with("https://")
}

/// Fetches the sheets the page couldn't read and its manifest's icons, within
/// `deadline` (the scan's start + [`FETCH_BUDGET`]); what fails or is late is left out.
/// A sheet's text goes back to the page, whose scripts can read it: only CSS, and from
/// this Mac or its network only for a page of that host (`page`, the tab's URL as Sawit
/// knows it), so a page can't read the dev server or the LAN through Sawit. The manifest
/// is asked for under the same host rule.
async fn fetch_extras(
    hrefs: Vec<String>,
    manifest: Option<String>,
    referer: String,
    page: String,
    deadline: std::time::Instant,
) -> Extras {
    let deadline = tokio::time::Instant::from_std(deadline);
    let (referer, page) = (referer.as_str(), page.as_str());
    let sheets = async {
        let wanted = hrefs.into_iter().filter(|h| {
            let ok = may_fetch_for(h, page);
            if is_http(h) && !ok {
                eprintln!("sawit: left out the stylesheet {h}: it is on this Mac or its network");
            }
            is_http(h) && ok
        });
        let mut stream = futures::stream::iter(wanted.enumerate().map(|(i, href)| async move {
            let text = fetch_text(&href, referer).await;
            (i, href, text)
        }))
        .buffer_unordered(FETCHES_AT_ONCE);
        let mut got = Vec::new();
        while let Ok(Some((i, href, text))) = tokio::time::timeout_at(deadline, stream.next()).await
        {
            match text.map(|f| sheet_for_page(&href, f, page)) {
                Ok(Ok(text)) => got.push((i, href, text)),
                Ok(Err(why)) => eprintln!("sawit: left out the stylesheet {href}: {why}"),
                Err(e) => eprintln!("sawit: couldn't fetch the stylesheet {href}: {e}"),
            }
        }
        got.sort_by_key(|(i, _, _)| *i);
        got.into_iter().map(|(_, h, t)| (h, t)).collect::<Vec<_>>()
    };
    let icons = async {
        let Some(manifest) = manifest.filter(|m| is_http(m)) else {
            return Vec::new();
        };
        if !may_fetch_for(&manifest, page) {
            eprintln!("sawit: left out the manifest {manifest}: it is on this Mac or its network");
            return Vec::new();
        }
        match tokio::time::timeout_at(deadline, fetch_text(&manifest, referer)).await {
            Ok(Ok(fetched)) => manifest_icons(&fetched.text, &manifest),
            Ok(Err(e)) => {
                eprintln!("sawit: couldn't fetch the manifest {manifest}: {e}");
                Vec::new()
            }
            Err(_) => Vec::new(),
        }
    };
    let (sheets, icons) = futures::join!(sheets, icons);
    Extras { sheets, icons }
}

/// Adds manifest icons the page didn't already list.
pub(super) fn add_icons(assets: &mut Vec<AssetRef>, icons: Vec<AssetRef>) {
    for icon in icons {
        if !assets.iter().any(|a| a.url == icon.url) {
            assets.push(icon);
        }
    }
}

/// The manifest's `icons[].src`, resolved against the manifest's URL.
pub(super) fn manifest_icons(text: &str, manifest_url: &str) -> Vec<AssetRef> {
    let Ok(manifest) = serde_json::from_str::<Value>(text) else {
        return Vec::new();
    };
    let Some(icons) = manifest["icons"].as_array() else {
        return Vec::new();
    };
    icons
        .iter()
        .filter_map(|icon| {
            let url = resolve_url(manifest_url, icon["src"].as_str()?)?;
            let size = icon["sizes"]
                .as_str()
                .and_then(|s| s.split_whitespace().next())
                .and_then(|s| {
                    s.to_ascii_lowercase()
                        .split_once('x')
                        .map(|(w, h)| (w.parse().ok(), h.parse().ok()))
                });
            let (width, height) = match size {
                Some((Some(w), Some(h))) => (Some(w), Some(h)),
                _ => (None, None),
            };
            let label = match (width, height) {
                (Some(w), Some(h)) => format!("app icon {w}x{h}"),
                _ => "app icon".to_string(),
            };
            Some(AssetRef {
                kind: AssetKind::Icon,
                url: Some(url),
                svg: None,
                label,
                width,
                height,
                used_on: vec![],
                logo: false,
            })
        })
        .collect()
}

/// `href` against `base` (an absolute http(s) URL); `None` for an empty `href` or a base
/// without an authority.
pub(super) fn resolve_url(base: &str, href: &str) -> Option<String> {
    let href = href.trim();
    if href.is_empty() {
        return None;
    }
    let has_scheme = href.split_once(':').is_some_and(|(scheme, _)| {
        scheme.starts_with(|c: char| c.is_ascii_alphabetic())
            && scheme
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
    });
    if has_scheme {
        return Some(href.to_string());
    }
    let (scheme, rest) = base.split_once("://")?;
    if let Some(rest) = href.strip_prefix("//") {
        return Some(format!("{scheme}://{rest}"));
    }
    let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let (authority, tail) = rest.split_at(end);
    let path = tail.split(['?', '#']).next().unwrap_or("");
    let origin = format!("{scheme}://{authority}");
    if href.starts_with('/') {
        return Some(format!("{origin}{href}"));
    }
    if href.starts_with(['?', '#']) {
        let path = if path.is_empty() { "/" } else { path };
        return Some(format!("{origin}{path}{href}"));
    }
    let dir = match path.rfind('/') {
        Some(i) => &path[..=i],
        None => "/",
    };
    Some(format!("{origin}{dir}{href}"))
}
