//! The side panel and picking: which panel is open, what a click on the page does for it
//! (Annotate notes the element, Design's Inspect reads it), Sawit's own keys (⇧⌘., Esc), and the
//! overlay's colours.

use futures::channel::oneshot;
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::*;
use sawit_core::browser::BrowserOp;
use serde_json::{Value, json};

use super::annotate::Picking;
use super::design::Segment;
use super::{BrowserView, OpResult, Panel, PickMode};

/// What picking does while `panel` is open: Design inspects only on its Inspect segment,
/// so clicks reach the page on the others.
pub(super) fn pick_mode(panel: Panel, segment: Segment) -> PickMode {
    match (panel, segment) {
        (Panel::Annotate, _) => PickMode::Annotate,
        (Panel::Design, Segment::Inspect) => PickMode::Inspect,
        (Panel::Design, _) => PickMode::Off,
    }
}

/// Whether a pick the page reports (`mode` as it says) is one Sawit armed: the page can arm
/// the picker itself, so a pick counts only in the mode armed, with its panel open.
pub(super) fn pick_wanted(panel: Option<Panel>, pick: PickMode, mode: &str) -> bool {
    match mode {
        "annotate" => panel == Some(Panel::Annotate) && pick == PickMode::Annotate,
        "inspect" => panel == Some(Panel::Design) && pick == PickMode::Inspect,
        _ => false,
    }
}

/// A key an agent presses (`browser_press_key`) that Sawit answers instead of the page.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum OwnKey {
    /// ⇧⌘.: toggles Annotate.
    ToggleAnnotate,
    /// Esc: stops picking, while picking.
    Escape,
}

/// Whether `key` (`Meta+Shift+.`, as `browser_press_key` takes it) is ⇧⌘. or Esc. The page
/// answers only the user's own Esc while picking, and no one's ⇧⌘. (a page can dispatch keys,
/// ⇧⌘. opens Annotate and arms the picker, and the user's own is the keymap's), so an agent's
/// go to Sawit. `>` is ⇧. on the keyboard.
pub(super) fn own_key(key: &str) -> Option<OwnKey> {
    let parts: Vec<String> = key
        .split('+')
        .map(|p| p.trim().to_ascii_lowercase())
        .collect();
    let (key, mods) = parts.split_last()?;
    let meta = mods
        .iter()
        .any(|m| matches!(m.as_str(), "meta" | "cmd" | "command"));
    let shift = mods.iter().any(|m| m == "shift");
    let others = mods
        .iter()
        .any(|m| !matches!(m.as_str(), "meta" | "cmd" | "command" | "shift"));
    match key.as_str() {
        "." if meta && shift && !others => Some(OwnKey::ToggleAnnotate),
        ">" if meta && !others => Some(OwnKey::ToggleAnnotate),
        // As the picker takes it: any Esc.
        "escape" | "esc" => Some(OwnKey::Escape),
        _ => None,
    }
}

impl BrowserView {
    /// Opens `panel`, or closes it when it is the one open. Annotate arms
    /// element picking, Design inspecting (on its Inspect segment); closing disarms.
    pub fn toggle_panel(&mut self, panel: Panel, cx: &mut Context<Self>) {
        if self.panel == Some(panel) {
            self.panel = None;
            self.set_pick(PickMode::Off, cx);
        } else {
            self.panel = Some(panel);
            let segment = self.design_panel.read(cx).segment();
            self.set_pick(pick_mode(panel, segment), cx);
            self.scan_if_design_open(cx);
        }
        let annotating = self.panel == Some(Panel::Annotate);
        self.annotate
            .update(cx, |a, cx| a.set_shown(annotating, cx));
        // mesurer is on the page while Annotate is open, and only then.
        if annotating {
            self.mesurer_open(cx);
        } else {
            self.mesurer_close(cx);
        }
        cx.notify();
    }

    /// `Panel`: opens `panel` (on Design's `tab`), or closes the panel open (`None`).
    pub(super) fn panel_op(
        &mut self,
        panel: Option<&str>,
        tab: Option<&str>,
        cx: &mut Context<Self>,
    ) -> OpResult {
        let want = match panel.map(str::to_ascii_lowercase).as_deref() {
            None | Some("none") => None,
            Some("annotate") => Some(Panel::Annotate),
            Some("design") => Some(Panel::Design),
            Some(other) => {
                return Err(format!(
                    "Unknown panel “{other}”. Use annotate or design, or leave it out to close."
                ));
            }
        };
        let segment = match tab {
            None => None,
            Some(name) => Some(Segment::named(name).ok_or_else(|| {
                format!("Unknown tab “{name}”. Use inspect, colors, fonts, assets or system.")
            })?),
        };
        if let Some(segment) = segment {
            self.design_panel
                .update(cx, |p, cx| p.set_segment(segment, cx));
        }
        match want {
            Some(want) if self.panel != Some(want) => self.toggle_panel(want, cx),
            Some(Panel::Design) => {
                let segment = self.design_panel.read(cx).segment();
                self.set_pick(pick_mode(Panel::Design, segment), cx);
            }
            Some(Panel::Annotate) => {}
            None => {
                if let Some(open) = self.panel {
                    self.toggle_panel(open, cx);
                }
            }
        }
        Ok(json!({
            "panel": self.panel.map(Panel::as_str),
            "tab": (self.panel == Some(Panel::Design))
                .then(|| self.design_panel.read(cx).segment().name()),
        }))
    }

    /// ⇧⌘.: like the Annotate button, except that with Annotate open after Esc
    /// it picks again instead of closing.
    pub(super) fn annotate_key(&mut self, cx: &mut Context<Self>) {
        if self.panel == Some(Panel::Annotate) && self.pick == PickMode::Off {
            self.set_pick(PickMode::Annotate, cx);
        } else {
            self.toggle_panel(Panel::Annotate, cx);
        }
    }

    /// An agent's ⇧⌘., or Esc while picking: does what the user's key does; the receiver gets
    /// where that left the panel and the picker. `None` for a key that is the page's. With
    /// mesurer on the page, an Esc while picking goes to the page first, as the user's does
    /// (`page/overlay/picking.js`): picking stops only if nothing there took it.
    pub(super) fn press_own_key(
        &mut self,
        key: &str,
        cx: &mut Context<Self>,
    ) -> Option<oneshot::Receiver<OpResult>> {
        match own_key(key)? {
            OwnKey::ToggleAnnotate => self.annotate_key(cx),
            OwnKey::Escape if self.pick != PickMode::Off && self.mesurer.mounted => {
                return Some(self.escape_through_page(key, cx));
            }
            OwnKey::Escape if self.pick != PickMode::Off => self.set_pick(PickMode::Off, cx),
            OwnKey::Escape => return None,
        }
        let (tx, rx) = oneshot::channel();
        let _ = tx.send(Ok(self.own_key_answer(key, json!({}))));
        Some(rx)
    }

    /// `answer`, with where the panel and the picker are now.
    fn own_key_answer(&self, key: &str, mut answer: Value) -> Value {
        if !answer.is_object() {
            answer = json!({});
        }
        answer["pressed"] = json!(key);
        answer["panel"] = json!(self.panel.map(Panel::as_str));
        answer["picking"] = json!(self.pick.as_str());
        answer
    }

    /// An agent's Esc, to the page (mesurer's tool, menu or toolbar take it first); then
    /// [`Self::escape_answered`].
    fn escape_through_page(
        &mut self,
        key: &str,
        cx: &mut Context<Self>,
    ) -> oneshot::Receiver<OpResult> {
        let (tx, rx) = oneshot::channel();
        let (page_tx, page_rx) = oneshot::channel();
        let op = BrowserOp::Press { key: key.into() };
        if self.engine.is_some() {
            self.run(op, page_tx, cx);
        } else {
            self.queued.push((op, page_tx));
        }
        let key = key.to_string();
        cx.spawn(async move |this, cx| {
            let pressed = page_rx
                .await
                .unwrap_or_else(|_| Err("The page didn't answer.".into()));
            let answer = this
                .update(cx, |this, cx| this.escape_answered(&key, pressed, cx))
                .unwrap_or_else(|_| Err("The browser closed.".into()));
            let _ = tx.send(answer);
        })
        .detach();
        rx
    }

    /// The page's answer to an agent's Esc: picking stops unless something there took the key
    /// (`prevented`, as the page agent reports it).
    pub(super) fn escape_answered(
        &mut self,
        key: &str,
        pressed: OpResult,
        cx: &mut Context<Self>,
    ) -> OpResult {
        let pressed = pressed?;
        let taken = pressed["prevented"] == json!(true);
        if !taken && self.pick != PickMode::Off {
            self.set_pick(PickMode::Off, cx);
        }
        let mut answer = self.own_key_answer(key, pressed);
        answer["taken_by_page"] = json!(taken);
        Ok(answer)
    }

    /// Arms (or disarms) picking on the page.
    pub(super) fn set_pick(&mut self, mode: PickMode, cx: &mut Context<Self>) {
        self.pick = mode;
        if mode == PickMode::Off {
            self.pick_waiting = false;
        } else {
            // The overlay takes Sawit's colours before it draws.
            drop(self.page_op(theme_op(cx)));
        }
        self.send_pick(pick_op(mode), cx);
        self.sync_picking(cx);
        cx.notify();
    }

    /// Tells the panels whether the page picks for them.
    fn sync_picking(&self, cx: &mut Context<Self>) {
        let picking = match (self.pick, self.pick_waiting) {
            (PickMode::Annotate, false) => Picking::On,
            (PickMode::Annotate, true) => Picking::Waiting,
            _ => Picking::Off,
        };
        self.annotate.update(cx, |a, cx| a.set_picking(picking, cx));
        let inspecting = self.pick == PickMode::Inspect;
        self.design_panel
            .update(cx, |p, cx| p.set_inspecting(inspecting, cx));
    }

    /// Sends a pick op; when the page has no agent yet, remembers to try
    /// again once it has loaded.
    pub(super) fn send_pick(&mut self, op: Value, cx: &mut Context<Self>) {
        let armed = self.pick;
        let reply = self.page_op(op);
        cx.spawn(async move |this, cx| {
            let failed = !matches!(reply.await, Ok(Ok(_)));
            let _ = this.update(cx, |this, cx| {
                if this.pick == armed {
                    this.pick_waiting = failed && armed != PickMode::Off;
                    this.sync_picking(cx);
                    cx.notify();
                }
            });
        })
        .detach();
    }
}

fn pick_op(mode: PickMode) -> Value {
    json!({ "op": "pick", "mode": mode.as_str() })
}

/// What a page needs after it loads: its picker armed again.
pub(super) fn rearm_ops(pick: PickMode) -> Vec<Value> {
    match pick {
        PickMode::Off => vec![],
        mode => vec![pick_op(mode)],
    }
}

/// Sawit's colours and faces for the page's overlay, so the popover, the markers and the
/// hover label look like the app around them.
pub(super) fn theme_op(cx: &App) -> Value {
    let t = cx.theme();
    let hex = |c: Hsla| {
        let c = c.to_rgb();
        let byte = |v: f32| (v.clamp(0., 1.) * 255.).round() as u8;
        if c.a >= 0.999 {
            format!("#{:02x}{:02x}{:02x}", byte(c.r), byte(c.g), byte(c.b))
        } else {
            format!(
                "#{:02x}{:02x}{:02x}{:02x}",
                byte(c.r),
                byte(c.g),
                byte(c.b),
                byte(c.a)
            )
        }
    };
    let face = |name: &str, fallback: &str| {
        // GPUI's own name for the system face (`.SystemUIFont`) means nothing to CSS.
        if name.is_empty() || name.starts_with('.') {
            fallback.to_string()
        } else {
            format!("\"{}\", {fallback}", name.replace('"', ""))
        }
    };
    json!({
        "op": "theme",
        "colors": {
            "bg": hex(t.popover),
            "fg": hex(t.popover_foreground),
            "muted": hex(t.muted_foreground),
            "border": hex(t.border),
            "field": hex(t.background),
            "accent": hex(t.secondary),
            "primary": hex(t.primary),
            "primaryFg": hex(t.primary_foreground),
            "danger": hex(t.danger),
            "ring": hex(t.ring),
            "font": face(&t.font_family, "-apple-system, BlinkMacSystemFont, system-ui, sans-serif"),
            "mono": face(&t.mono_font_family, "ui-monospace, Menlo, monospace"),
        }
    })
}
