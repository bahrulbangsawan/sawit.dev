//! The machine checks of a UI screen: its `index.html` with the style sheets it holds and links
//! from its folder, read as a browser would cascade them ([`style`]), each element checked
//! against the rules a program can check ([`check`]).

mod sheet;
mod style;

use std::path::Path;

use html5ever::Attribute;
use markup5ever_rcdom::{Handle, NodeData};

use super::css::{El, Sheet};
use super::{
    Finding, MIN_INPUT, MIN_INPUT_TOUCH, MIN_TARGET, MIN_TARGET_TOUCH, MIN_TEXT, TOUCH_WIDTH,
    contrast, contrast_fix, dedup, needed_contrast, size_fix, target_fix,
};
use crate::screens::inline::{HTML_NS, attr, parse};
use crate::screens::{INDEX, has_scheme, read};
use sheet::{focus, spacing};
use style::{Cascaded, Computed, Inherited};

/// Average glyph width as a share of the font size, to guess a text's width.
const GLYPH: f64 = 0.5;

/// Checks screen folder `folder`, laid out `width` CSS px wide.
pub fn check_folder(folder: &Path, width: f64) -> Result<Vec<Finding>, String> {
    let page = read(folder, INDEX).map_err(|e| format!("couldn't read {INDEX}: {e}"))?;
    let html = String::from_utf8_lossy(&page);
    Ok(check(&html, &|rel| read(folder, rel).ok(), width))
}

/// The failures of page `html` (its linked files read with `read`), laid out `width` px wide:
/// one per rule and element, in document order.
pub fn check(html: &str, read: &dyn Fn(&str) -> Option<Vec<u8>>, width: f64) -> Vec<Finding> {
    let dom = parse(html);
    let mut sheet = Sheet::default();
    let mut labels = Vec::new();
    gather(&dom.document, &mut sheet, &mut labels, read);
    let mut walk = Walk {
        sheet: &sheet,
        labels: &labels,
        touch: width < TOUCH_WIDTH,
        findings: Vec::new(),
        interactive: false,
    };
    walk.node(&dom.document, &mut Vec::new(), &Inherited::default(), false);
    let mut findings = walk.findings;
    let interactive = walk.interactive;
    spacing(&sheet, &mut findings);
    focus(&sheet, interactive, &mut findings);
    dedup(findings)
}

/// The page's style sheets (`<style>` and local `<link rel=stylesheet>`, in order) and the ids
/// its `<label for>`s name.
fn gather(
    node: &Handle,
    sheet: &mut Sheet,
    labels: &mut Vec<String>,
    read: &dyn Fn(&str) -> Option<Vec<u8>>,
) {
    if let NodeData::Element { name, attrs, .. } = &node.data {
        let attrs = attrs.borrow();
        match &*name.local {
            "style" => sheet.add(&text(node)),
            "link" if rel_stylesheet(&attrs) => {
                let href = attr(&attrs, "href").unwrap_or_default();
                if !href.is_empty() && !has_scheme(href) {
                    let rel = href.split(['?', '#']).next().unwrap_or_default();
                    if let Some(bytes) = read(rel.trim_start_matches("./")) {
                        sheet.add(&String::from_utf8_lossy(&bytes));
                    }
                }
            }
            "label" => labels.extend(attr(&attrs, "for").map(str::to_string)),
            _ => {}
        }
    }
    for child in node.children.borrow().iter() {
        gather(child, sheet, labels, read);
    }
}

fn rel_stylesheet(attrs: &[Attribute]) -> bool {
    let rel = attr(attrs, "rel").unwrap_or_default().to_ascii_lowercase();
    rel.split_whitespace().any(|r| r == "stylesheet")
}

/// The text of `node`'s own text children.
fn text(node: &Handle) -> String {
    let mut out = String::new();
    for child in node.children.borrow().iter() {
        if let NodeData::Text { contents } = &child.data {
            out.push_str(&contents.borrow());
        }
    }
    out
}

/// All the text under `node`, and whether an image with alt text is there.
fn deep_text(node: &Handle, out: &mut String) -> bool {
    let mut named = false;
    for child in node.children.borrow().iter() {
        match &child.data {
            NodeData::Text { contents } => out.push_str(&contents.borrow()),
            NodeData::Element { name, attrs, .. } => {
                let attrs = attrs.borrow();
                let alt = attr(&attrs, "alt").or(attr(&attrs, "aria-label"));
                named |= matches!(&*name.local, "img" | "svg")
                    && alt.is_some_and(|a| !a.trim().is_empty());
                named |= deep_text(child, out);
            }
            _ => {}
        }
    }
    named
}

struct Walk<'a> {
    sheet: &'a Sheet,
    labels: &'a [String],
    touch: bool,
    findings: Vec<Finding>,
    interactive: bool,
}

impl Walk<'_> {
    fn node(&mut self, node: &Handle, path: &mut Vec<El>, parent: &Inherited, in_label: bool) {
        let NodeData::Element { name, attrs, .. } = &node.data else {
            for child in node.children.borrow().iter() {
                self.node(child, path, parent, in_label);
            }
            return;
        };
        if &*name.ns != HTML_NS {
            return;
        }
        let tag = name.local.to_string();
        if matches!(
            tag.as_str(),
            "head" | "script" | "style" | "template" | "noscript"
        ) {
            return;
        }
        let attrs = attrs.borrow();
        if attr(&attrs, "hidden").is_some() {
            return;
        }
        let el = element(&tag, &attrs);
        path.push(el);
        let cascaded = Cascaded::of(self.sheet, path, attr(&attrs, "style"));
        let computed = Computed::new(&tag, cascaded, parent);
        if !computed.hidden {
            let selector = describe(path);
            self.element(node, &tag, &attrs, &computed, &selector, in_label);
            let in_label = in_label || tag == "label";
            for child in node.children.borrow().iter() {
                self.node(child, path, &computed.inherited, in_label);
            }
        }
        path.pop();
    }

    /// The checks of one element.
    fn element(
        &mut self,
        node: &Handle,
        tag: &str,
        attrs: &[Attribute],
        c: &Computed,
        selector: &str,
        in_label: bool,
    ) {
        let own = text(node);
        let has_text = !own.trim().is_empty();
        let disabled = attr(attrs, "disabled").is_some();
        let input_type = attr(attrs, "type").unwrap_or("text").to_ascii_lowercase();
        let field = match tag {
            "input" => !matches!(
                input_type.as_str(),
                "hidden" | "submit" | "reset" | "button" | "image" | "checkbox" | "radio"
            ),
            "select" | "textarea" => true,
            _ => false,
        };
        let i = &c.inherited;
        if (has_text || field) && !disabled && !i.bg_image {
            let fg = super::Rgba {
                a: i.color.a * i.opacity,
                ..i.color
            };
            let ratio = contrast(fg, i.bg);
            let needed = needed_contrast(i.font_size, i.weight);
            if ratio < needed {
                let fix = contrast_fix(fg, i.bg, ratio, needed);
                self.findings
                    .push(Finding::at_selector("col.10", selector, fix));
            }
        }
        if has_text && i.font_size < MIN_TEXT - 1e-6 {
            let fix = size_fix(i.font_size, MIN_TEXT);
            self.findings
                .push(Finding::at_selector("typ.11", selector, fix));
        } else if field {
            let floor = if self.touch {
                MIN_INPUT_TOUCH
            } else {
                MIN_INPUT
            };
            if i.font_size < floor - 0.01 {
                let fix = size_fix(i.font_size, floor);
                self.findings
                    .push(Finding::at_selector("typ.11", selector, fix));
            }
        }
        if tag == "img" && attr(attrs, "alt").is_none() {
            let fix = "The image has no alt attribute: say what it shows, or alt=\"\" when it is \
                       only decoration.";
            self.findings
                .push(Finding::at_selector("a11y.7", selector, fix));
        }
        if field && !self.labelled(attrs, in_label) {
            let fix = "The field has no label: add a <label for> (a placeholder isn't one), or \
                       aria-label when the label can't show.";
            self.findings
                .push(Finding::at_selector("a11y.8", selector, fix));
        }
        let link = tag == "a" && attr(attrs, "href").is_some();
        let control = link
            || matches!(tag, "button" | "select" | "textarea" | "summary")
            || (tag == "input" && input_type != "hidden")
            || attr(attrs, "role").is_some_and(|r| r == "button" || r == "link");
        if control {
            self.interactive = true;
            self.target(node, tag, &input_type, c, selector, link);
            self.name(node, tag, attrs, selector);
        }
        if has_text {
            self.overflow(&own, c, selector);
        }
    }

    /// Whether field `attrs` has a label.
    fn labelled(&self, attrs: &[Attribute], in_label: bool) -> bool {
        let named = |a: &str| attr(attrs, a).is_some_and(|v| !v.trim().is_empty());
        in_label
            || named("aria-label")
            || named("aria-labelledby")
            || named("title")
            || attr(attrs, "id").is_some_and(|id| self.labels.iter().any(|l| l == id))
    }

    /// a11y.2: a control's target size, its own or made by its text and padding.
    fn target(
        &mut self,
        node: &Handle,
        tag: &str,
        kind: &str,
        c: &Computed,
        selector: &str,
        link: bool,
    ) {
        // The browser sizes checkboxes and radios, and a link inside a sentence flows with it.
        if tag == "input" && matches!(kind, "checkbox" | "radio") && c.px("width").is_none() {
            return;
        }
        if link && inline_in_text(node) {
            return;
        }
        let i = &c.inherited;
        let pad = c.sides("padding");
        let border = c.sides("border");
        let line = i.line_height.unwrap_or(i.font_size * 1.2);
        let height = match (c.px("height"), c.px("min-height")) {
            (None, None) => line + pad[0] + pad[2] + border[0] + border[2],
            (h, m) => h.unwrap_or(0.0).max(m.unwrap_or(0.0)),
        };
        let mut words = String::new();
        deep_text(node, &mut words);
        let block = c.get("width").is_some_and(|w| w.contains('%'))
            || c.get("display").is_some_and(|d| {
                d.contains("block") || d.contains("flex") && !d.contains("inline")
            });
        let width = match (c.px("width"), c.px("min-width")) {
            (None, None) if block || matches!(tag, "select" | "textarea") || tag == "input" => None,
            (None, None) => Some(
                words.trim().chars().count() as f64 * GLYPH * i.font_size
                    + pad[1]
                    + pad[3]
                    + border[1]
                    + border[3],
            ),
            (w, m) => Some(w.unwrap_or(0.0).max(m.unwrap_or(0.0))),
        };
        let floor = if self.touch {
            MIN_TARGET_TOUCH
        } else {
            MIN_TARGET
        };
        let short = height < floor - 0.01;
        let narrow = width.is_some_and(|w| w < floor - 0.01);
        if short || narrow {
            let fix = target_fix(width.unwrap_or(height), height, floor);
            self.findings
                .push(Finding::at_selector("a11y.2", selector, fix));
        }
    }

    /// a11y.7: an icon-only button or link has a name.
    fn name(&mut self, node: &Handle, tag: &str, attrs: &[Attribute], selector: &str) {
        if !matches!(tag, "button" | "a") {
            return;
        }
        let mut words = String::new();
        let named = deep_text(node, &mut words)
            || !words.trim().is_empty()
            || ["aria-label", "aria-labelledby", "title"]
                .iter()
                .any(|a| attr(attrs, a).is_some_and(|v| !v.trim().is_empty()));
        if !named {
            let fix = "The control shows no text and has no aria-label: name it with a verb and \
                       its object (\"Delete project\").";
            self.findings
                .push(Finding::at_selector("a11y.7", selector, fix));
        }
    }

    /// lay.18: text that can't fit the fixed box it is in.
    fn overflow(&mut self, text: &str, c: &Computed, selector: &str) {
        let Some(width) = c.px("width") else {
            return;
        };
        let scrolls = c
            .get("overflow")
            .is_some_and(|o| o.contains("auto") || o.contains("scroll"));
        if scrolls {
            return;
        }
        let i = &c.inherited;
        let (pad, border) = (c.sides("padding"), c.sides("border"));
        let room = (width - pad[1] - pad[3] - border[1] - border[3]).max(1.0);
        let wide = text
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .chars()
            .count() as f64
            * GLYPH
            * i.font_size;
        let ellipsis = c
            .get("text-overflow")
            .is_some_and(|t| t.contains("ellipsis"))
            || c.get("-webkit-line-clamp").is_some()
            || c.get("line-clamp").is_some();
        if ellipsis {
            return;
        }
        let fix = if i.nowrap {
            (wide > room).then(|| {
                format!(
                    "The text runs about {}px on one line in a box with {}px for it: let it \
                     wrap, or end it in an ellipsis with a way to the full text (typ.16).",
                    wide.round(),
                    room.round()
                )
            })
        } else {
            let Some(height) = c.px("height") else {
                return;
            };
            let line = i.line_height.unwrap_or(i.font_size * 1.2);
            let lines = (wide / room).ceil().max(1.0);
            let needed = lines * line + pad[0] + pad[2] + border[0] + border[2];
            (needed > height + 1.0).then(|| {
                format!(
                    "The text needs about {lines} lines ({}px) in a box {}px high: use \
                     min-height instead of height so it grows, or shorten the copy.",
                    needed.round(),
                    height.round()
                )
            })
        };
        if let Some(fix) = fix {
            self.findings
                .push(Finding::at_selector("lay.18", selector, fix));
        }
    }
}

/// Whether link `node` sits in a run of text: its parent holds text of its own.
fn inline_in_text(node: &Handle) -> bool {
    let parent = node.parent.take();
    node.parent.set(parent.clone());
    let Some(parent) = parent.and_then(|p| p.upgrade()) else {
        return false;
    };
    let words = text(&parent);
    !words.trim().is_empty()
}

/// Element `tag` with `attrs` as selectors see it.
fn element(tag: &str, attrs: &[Attribute]) -> El {
    El {
        tag: tag.to_string(),
        id: attr(attrs, "id").map(str::to_string),
        classes: attr(attrs, "class")
            .unwrap_or_default()
            .split_whitespace()
            .map(str::to_string)
            .collect(),
        attrs: attrs
            .iter()
            .filter(|a| a.name.ns.is_empty())
            .map(|a| (a.name.local.to_string(), a.value.to_string()))
            .collect(),
    }
}

/// A selector that finds `path`'s last element: `tag#id`, `tag.class`, or its parent's then it.
fn describe(path: &[El]) -> String {
    let one = |el: &El| match (&el.id, el.classes.is_empty()) {
        (Some(id), _) => format!("{}#{id}", el.tag),
        (None, false) => format!("{}.{}", el.tag, el.classes.join(".")),
        (None, true) => el.tag.clone(),
    };
    let Some((last, rest)) = path.split_last() else {
        return String::new();
    };
    let named = last.id.is_some() || !last.classes.is_empty();
    match rest.last().filter(|_| !named) {
        Some(parent) => format!("{} > {}", one(parent), one(last)),
        None => one(last),
    }
}
