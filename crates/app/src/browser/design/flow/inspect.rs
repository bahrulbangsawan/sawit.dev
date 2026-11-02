//! Inspect: an element's styles from the page's `style` op; for the panel's hovers and picks,
//! only the latest answer counts.

use gpui_kit::{Context, Task};
use sawit_core::site_design::inspect::StyleInfo;
use serde_json::{Value, json};

use super::{page_answer, parse};
use crate::browser::BrowserView;

/// What a dropped (superseded) inspect answers; the panel ignores it.
pub const STALE_INSPECT: &str = "A newer inspect replaced this one.";

impl BrowserView {
    /// One element's styles; `html` adds its outerHTML with styles inlined.
    pub(in crate::browser) fn style_info(
        &self,
        target: Value,
        html: bool,
        cx: &mut Context<Self>,
    ) -> Task<Result<StyleInfo, String>> {
        let answer = self.page_op(style_op(target, html));
        cx.spawn(async move |_, cx| {
            parse(
                "style",
                serde_json::from_value(page_answer(answer, cx).await?),
            )
        })
    }

    /// Like [`Self::style_info`], latest only: an answer overtaken by a newer inspect errs
    /// with [`STALE_INSPECT`].
    pub(crate) fn inspect(
        &mut self,
        target: Value,
        html: bool,
        cx: &mut Context<Self>,
    ) -> Task<Result<StyleInfo, String>> {
        self.design.inspect_seq += 1;
        let seq = self.design.inspect_seq;
        let info = self.style_info(target, html, cx);
        cx.spawn(async move |this, cx| {
            let info = info.await;
            let latest = this
                .read_with(cx, |this, _| is_latest(seq, this.design.inspect_seq))
                .unwrap_or(false);
            if !latest {
                return Err(STALE_INSPECT.into());
            }
            info
        })
    }

    /// The pointer is on an element in inspect mode (`pin: false`), or a click picked it
    /// (`pin: true`): reads its styles for the Design panel.
    pub(in crate::browser) fn inspect_element(
        &mut self,
        selector: String,
        pin: bool,
        cx: &mut Context<Self>,
    ) {
        // Hovers still on their way after picking stopped are dropped.
        if self.panel != Some(crate::browser::Panel::Design)
            || self.pick != crate::browser::PickMode::Inspect
        {
            return;
        }
        let pinned = self.design_panel.read(cx).is_pinned();
        if !should_inspect(
            pinned,
            self.design.pins_pending,
            self.design.hover_req.as_deref(),
            &selector,
            pin,
        ) {
            return;
        }
        if pin {
            self.design.pins_pending += 1;
            self.design.hover_req = None;
        } else {
            self.design.hover_req = Some(selector.clone());
        }
        let info = self.inspect(json!({ "selector": selector }), false, cx);
        cx.spawn(async move |this, cx| {
            let info = info.await;
            let _ = this.update(cx, |this, cx| {
                if pin {
                    this.design.pins_pending = this.design.pins_pending.saturating_sub(1);
                }
                match info {
                    Ok(info) => this.design_panel.update(cx, |panel, cx| {
                        if pin {
                            panel.pin(info, cx);
                        } else {
                            panel.set_hovered(info, cx);
                        }
                    }),
                    // Overtaken: the newer ask answers. A failure lets the same element be
                    // asked again.
                    Err(e) => {
                        if e != STALE_INSPECT && !pin && this.design.hover_req.is_some() {
                            this.design.hover_req = None;
                        }
                    }
                }
            });
        })
        .detach();
    }
}

/// Whether a hover (`pin: false`) or a pick (`pin: true`) of `selector` asks the page. A pick
/// always does. A hover waits while an element is pinned or a pick is pending (it would
/// overtake the pick's answer), and repeats nothing: the last element asked about is the one
/// the answer in flight, or on show, is for.
pub(super) fn should_inspect(
    pinned: bool,
    pins_pending: u32,
    last_hover: Option<&str>,
    selector: &str,
    pin: bool,
) -> bool {
    pin || (!pinned && pins_pending == 0 && last_hover != Some(selector))
}

pub(super) fn is_latest(seq: u64, current: u64) -> bool {
    seq == current
}

pub(super) fn style_op(target: Value, html: bool) -> Value {
    let mut op = match target {
        Value::Object(map) => Value::Object(map),
        _ => json!({}),
    };
    op["op"] = "style".into();
    op["html"] = html.into();
    op
}
