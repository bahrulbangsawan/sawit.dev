//! The page's mesurer markup for the notes ([`PageMarkup`]), its marks named by the elements under
//! them. The page's agent names them (`elements_at`) a moment after each save while mesurer is on
//! the page, and the answers are kept by where each mark is, so the notes, which are written at
//! once (Copy, Send, `browser_annotations`), take them as they are.

use std::time::Duration;

use gpui_kit::*;
use sawit_core::annotations::{MarkPoint, Near, PageMarkup};
use sawit_core::browser::mesurer::{origin_key, page_artifacts, page_key};
use serde_json::{Value, json};

use super::super::super::BrowserView;
use super::super::super::shots::within;
use super::super::CALL_TIMEOUT;
use super::here;

/// How long after a save the marks are looked at: mesurer saves as a drag goes on.
const RESOLVE_AFTER: Duration = Duration::from_millis(300);

/// Looks, a moment from now, for the elements under the marks of the page on show that aren't
/// named yet. The page's agent names them, mesurer or not: a mount's look comes before mesurer is
/// on the page.
pub(super) fn resolve_soon(view: &mut BrowserView, cx: &mut Context<BrowserView>) {
    if view.engine.is_none() {
        return;
    }
    view.mesurer.agents.resolving = Some(cx.spawn(async move |this, cx| {
        cx.background_executor().timer(RESOLVE_AFTER).await;
        let Ok(Some((page, asked, answer))) = this.update(cx, |this, _| {
            let (url, origin, key) = here(this);
            let state = &mut this.mesurer.agents;
            let page = (origin.clone(), key);
            if state.near_page.as_ref() != Some(&page) {
                state.near.clear();
                state.near_page = Some(page.clone());
            }
            let markup = this.mesurer_markup(&url)?;
            let near = &this.mesurer.agents.near;
            let asked: Vec<MarkPoint> = (markup.points().into_iter())
                .filter(|p| near.get(&p.key).is_none_or(Option::is_none))
                .collect();
            if asked.is_empty() {
                return None;
            }
            let points: Vec<Value> = asked
                .iter()
                .map(|p| json!({ "x": p.x, "y": p.y }))
                .collect();
            let answer = this.page_op(json!({ "op": "elements_at", "points": points }));
            Some((page, asked, answer))
        }) else {
            return;
        };
        let timer = cx.background_executor().timer(CALL_TIMEOUT);
        let Some(Ok(Ok(answer))) = within(answer, timer).await else {
            return;
        };
        let found = answer["elements"].as_array().cloned().unwrap_or_default();
        let _ = this.update(cx, |this, _| {
            let state = &mut this.mesurer.agents;
            if state.near_page.as_ref() != Some(&page) {
                return;
            }
            for (point, element) in asked.into_iter().zip(found) {
                let near = serde_json::from_value::<Near>(element).ok();
                state.near.insert(point.key, near);
            }
        });
    }));
}

impl BrowserView {
    /// What mesurer has drawn on the page at `url` besides its comments, from the workspace held
    /// (`None` while none is, or nothing is drawn). The page on show's marks are named as far as
    /// the page's agent has named them, and it carries X-ray and the session's captures.
    pub(in crate::browser) fn mesurer_markup(&self, url: &str) -> Option<PageMarkup> {
        let origin = origin_key(url);
        let envelope = self.mesurer.store.workspace(&origin)?;
        let key = page_key(url);
        let current = self.current_url();
        let on_show = origin == origin_key(&current) && key == page_key(&current);
        let xray = on_show && envelope["xrayVisible"] == true;
        let mut markup = PageMarkup::of(&page_artifacts(envelope, &key), xray);
        if on_show {
            let state = &self.mesurer.agents;
            if state.near_page.as_ref() == Some(&(origin, key)) {
                markup.place(&state.near);
            }
            markup.captures = self.mesurer.captures.clone();
        }
        (!markup.is_empty()).then_some(markup)
    }
}
