//! The page's `<body>` as JSX: each element on its own line, two spaces deeper per level, text
//! escaped for JSX, comments as `{/* … */}`.
//!
//! JSX drops the whitespace around a line break, where HTML renders it as a space, so the space
//! a page shows is written out: runs of whitespace collapse to one space as HTML renders them,
//! whitespace at a block's edges and between blocks goes, and a space that shows between inline
//! content is written `{" "}`. Inside `<pre>` text is kept exactly, as string literals.

use std::fmt::Write as _;

use html5ever::QualName;
use markup5ever_rcdom::{Handle, NodeData};

use super::attrs::{Prop, Value};
use super::{Convert, js_string};
use crate::screens::inline::{HTML_NS, SVG_NS};

/// HTML elements laid out as blocks by default: whitespace at their edges, and between them,
/// doesn't show. `<br>` ends a line, so whitespace around it doesn't show either.
const BLOCKS: &[&str] = &[
    "address",
    "article",
    "aside",
    "blockquote",
    "body",
    "br",
    "caption",
    "center",
    "col",
    "colgroup",
    "dd",
    "details",
    "dialog",
    "dir",
    "div",
    "dl",
    "dt",
    "fieldset",
    "figcaption",
    "figure",
    "footer",
    "form",
    "frameset",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "head",
    "header",
    "hgroup",
    "hr",
    "html",
    "legend",
    "li",
    "listing",
    "main",
    "menu",
    "nav",
    "ol",
    "optgroup",
    "option",
    "p",
    "pre",
    "search",
    "section",
    "summary",
    "table",
    "tbody",
    "td",
    "tfoot",
    "th",
    "thead",
    "tr",
    "ul",
];

/// HTML elements whose own text never shows (React refuses some of it): whitespace in them goes.
const NO_TEXT: &[&str] = &[
    "audio", "colgroup", "datalist", "dl", "html", "map", "object", "ol", "optgroup", "picture",
    "select", "table", "tbody", "tfoot", "thead", "tr", "ul", "video",
];

/// SVG elements whose text shows.
const SVG_TEXT: &[&str] = &["text", "tspan", "textPath", "title", "desc"];

/// A child of an element, as the JSX writes it.
#[derive(Clone, Debug)]
enum Item {
    Element(Handle),
    Text(String),
    Comment(String),
}

/// What an element's children are written within.
#[derive(Clone, Copy, Debug)]
struct Context {
    /// In `<pre>`: whitespace kept exactly.
    pre: bool,
    /// The element is a block: whitespace at its edges doesn't show.
    block: bool,
    /// The element's own text never shows.
    no_text: bool,
}

impl Context {
    /// The context of element `name`'s children, inside a `<pre>` or not.
    fn of(name: &QualName, pre: bool) -> Context {
        let tag = &*name.local;
        let html = &*name.ns == HTML_NS;
        let svg = &*name.ns == SVG_NS;
        Context {
            pre: pre || (html && matches!(tag, "pre" | "listing" | "plaintext")),
            // A button is an inline block: whitespace at its edges doesn't show either.
            block: (html && tag != "br" && (BLOCKS.contains(&tag) || tag == "button"))
                || (svg && matches!(tag, "text" | "title" | "desc")),
            no_text: (html && NO_TEXT.contains(&tag)) || (svg && !SVG_TEXT.contains(&tag)),
        }
    }
}

impl Convert<'_> {
    /// Writes the JSX the component returns: the body's children in a fragment, or in a `<div>`
    /// with the body's attributes when it has any.
    pub(super) fn root(&mut self, body: Option<&Handle>, out: &mut String) {
        let props = body.map(|body| self.props(body)).unwrap_or_default();
        let context = Context {
            pre: false,
            block: true,
            no_text: false,
        };
        let items = body
            .map(|body| self.items(body, context))
            .unwrap_or_default();
        let (open, close) = match props.is_empty() {
            true => ("<>".to_string(), "</>"),
            false => {
                self.note(
                    "The <body>'s attributes are on the <div> that wraps the component.".into(),
                );
                (format!("<div{}>", props_text(&props)), "</div>")
            }
        };
        let _ = writeln!(out, "    {open}");
        self.write_items(&items, 3, false, out);
        let _ = writeln!(out, "    {close}");
    }

    /// `parent`'s children as items: what isn't JSX taken out ([`Convert::stays`]), text next to
    /// text joined, and whitespace settled as HTML renders it.
    fn items(&mut self, parent: &Handle, context: Context) -> Vec<Item> {
        let mut items: Vec<Item> = Vec::new();
        self.gather(parent, &mut items);
        match context.pre {
            true => {
                items.retain(|item| !matches!(item, Item::Text(text) if text.is_empty()));
                items
            }
            false => settle(&items, context),
        }
    }

    /// Adds `parent`'s children to `items`, text joined to the text before it. An element whose
    /// tag JSX can't write (`<o:p>`) is left out, its children in its place.
    fn gather(&mut self, parent: &Handle, items: &mut Vec<Item>) {
        for child in children(parent) {
            match &child.data {
                NodeData::Text { contents } => match items.last_mut() {
                    Some(Item::Text(text)) => text.push_str(&contents.borrow()),
                    _ => items.push(Item::Text(contents.borrow().to_string())),
                },
                NodeData::Comment { contents } => items.push(Item::Comment(contents.to_string())),
                NodeData::Element { name, .. } if self.stays(&child) => {
                    if is_tag(&name.local) {
                        items.push(Item::Element(child.clone()));
                        continue;
                    }
                    let shown: String = name
                        .local
                        .chars()
                        .filter(|c| !c.is_control())
                        .take(40)
                        .collect();
                    self.note(format!(
                        "<{shown}> isn't a tag JSX can write, so its content stands in its place."
                    ));
                    self.gather(&child, items);
                }
                _ => {}
            }
        }
    }

    /// Writes `items` at `depth`, one to a line.
    fn write_items(&mut self, items: &[Item], depth: usize, pre: bool, out: &mut String) {
        for item in items {
            match item {
                Item::Element(node) => self.element(node, depth, pre, out),
                Item::Text(text) => {
                    let _ = writeln!(out, "{}{}", indent(depth), text_jsx(text, pre));
                }
                Item::Comment(text) => {
                    let _ = writeln!(out, "{}{}", indent(depth), comment_jsx(text));
                }
            }
        }
    }

    /// Writes element `node` at `depth`: self-closed when it has no children, on one line when
    /// its one child is text.
    fn element(&mut self, node: &Handle, depth: usize, pre: bool, out: &mut String) {
        let NodeData::Element { name, .. } = &node.data else {
            return;
        };
        let tag = name.local.to_string();
        let context = Context::of(name, pre);
        if &*name.ns == HTML_NS && tag.contains('-') && !self.custom.contains(&tag) {
            self.custom.push(tag.clone());
        }
        let props = self.props(node);
        let textarea = &*name.ns == HTML_NS && tag == "textarea";
        let items = match textarea {
            true => Vec::new(),
            false => self.items(node, context),
        };
        let _ = write!(out, "{}<{tag}{}", indent(depth), props_text(&props));
        match items.as_slice() {
            [] => out.push_str(" />\n"),
            [Item::Text(text)] => {
                let _ = writeln!(out, ">{}</{tag}>", text_jsx(text, context.pre));
            }
            _ => {
                out.push_str(">\n");
                self.write_items(&items, depth + 1, context.pre, out);
                let _ = writeln!(out, "{}</{tag}>", indent(depth));
            }
        }
    }
}

/// `node`'s children: a `<template>`'s are its content's.
fn children(node: &Handle) -> Vec<Handle> {
    if let NodeData::Element {
        template_contents, ..
    } = &node.data
        && let Some(content) = template_contents.borrow().as_ref()
    {
        return content.children.borrow().clone();
    }
    node.children.borrow().clone()
}

/// `items` with their text as HTML renders it: whitespace runs as one space, the space at a
/// block's edges, before or after a block or a `<br>` gone, text that is then empty gone, and
/// whitespace where text never shows gone. Comments don't count as neighbours.
fn settle(items: &[Item], context: Context) -> Vec<Item> {
    let edge = |neighbour: Option<&Item>| match neighbour {
        None => context.block,
        Some(Item::Element(node)) => is_block(node),
        Some(_) => false,
    };
    let mut settled = Vec::with_capacity(items.len());
    for (at, item) in items.iter().enumerate() {
        let Item::Text(text) = item else {
            settled.push(item.clone());
            continue;
        };
        let collapsed = collapse(text);
        if context.no_text && collapsed.trim_matches(' ').is_empty() {
            continue;
        }
        let shown = |item: &&Item| !matches!(item, Item::Comment(_));
        let before = items[..at].iter().rev().find(shown);
        let after = items[at + 1..].iter().find(shown);
        let mut text = collapsed.as_str();
        if edge(before) {
            text = text.trim_start_matches(' ');
        }
        if edge(after) {
            text = text.trim_end_matches(' ');
        }
        if !text.is_empty() {
            settled.push(Item::Text(text.to_string()));
        }
    }
    settled
}

/// Whether `node` is a block, or a `<br>`: whitespace next to it doesn't show.
fn is_block(node: &Handle) -> bool {
    match &node.data {
        NodeData::Element { name, .. } => &*name.ns == HTML_NS && BLOCKS.contains(&&*name.local),
        _ => false,
    }
}

/// `text` with each run of HTML whitespace (space, tab, line breaks, form feed) as one space.
fn collapse(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut space = false;
    for c in text.chars() {
        if matches!(c, ' ' | '\t' | '\n' | '\r' | '\x0c') {
            space = true;
            continue;
        }
        if space {
            out.push(' ');
            space = false;
        }
        out.push(c);
    }
    if space {
        out.push(' ');
    }
    out
}

/// Text as JSX: in `<pre>` one string literal; else escaped, with a space at either end written
/// `{" "}`, since JSX drops it at a line's ends.
fn text_jsx(text: &str, pre: bool) -> String {
    if pre {
        return format!("{{{}}}", js_string(text));
    }
    const SPACE: &str = "{\" \"}";
    let body = text.trim_matches(' ');
    if body.is_empty() {
        return SPACE.into();
    }
    let mut out = String::with_capacity(text.len() + 8);
    if text.starts_with(' ') {
        out.push_str(SPACE);
    }
    for c in body.chars() {
        match c {
            '{' => out.push_str("{\"{\"}"),
            '}' => out.push_str("{\"}\"}"),
            '<' => out.push_str("{\"<\"}"),
            '>' => out.push_str("{\">\"}"),
            '&' => out.push_str("&amp;"),
            '\u{a0}' => out.push_str("&nbsp;"),
            c => out.push(c),
        }
    }
    if text.ends_with(' ') {
        out.push_str(SPACE);
    }
    out
}

/// A comment as JSX, its `*/` made harmless.
fn comment_jsx(text: &str) -> String {
    format!("{{/* {} */}}", text.trim().replace("*/", "* /"))
}

/// Props as JSX attributes, each after a space. A name JSX can't write (`@click`, `x-on:click`)
/// goes in a spread object: `{...{ "@click": "…" }}`.
pub(super) fn props_text(props: &[Prop]) -> String {
    let mut out = String::new();
    for prop in props {
        out.push(' ');
        if !is_attr_name(&prop.name) {
            let value = match &prop.value {
                Value::Bare => "true".to_string(),
                Value::Str(text) => js_string(text),
                Value::Expr(expr) => expr.clone(),
            };
            let _ = write!(out, "{{...{{ {}: {value} }}}}", js_string(&prop.name));
            continue;
        }
        let _ = match &prop.value {
            Value::Bare => write!(out, "{}", prop.name),
            Value::Str(text) if plain(text) => write!(out, "{}=\"{text}\"", prop.name),
            Value::Str(text) => write!(out, "{}={{{}}}", prop.name, js_string(text)),
            Value::Expr(expr) => write!(out, "{}={{{expr}}}", prop.name),
        };
    }
    out
}

/// Whether `text` can be a JSX attribute string as it is: no `"`, no `&` (JSX would read an
/// entity), no `\` and no control characters.
fn plain(text: &str) -> bool {
    !text
        .chars()
        .any(|c| matches!(c, '"' | '&' | '\\' | '\u{2028}' | '\u{2029}') || c.is_control())
}

/// Whether `name` is a JSX attribute name: a JavaScript identifier, `-` allowed after its start.
fn is_attr_name(name: &str) -> bool {
    name.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_' || c == '$')
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '$' | '-'))
}

/// Whether `tag` can be written as an intrinsic JSX element: starts with a lowercase letter, then
/// letters, digits, `-` and `_`.
fn is_tag(tag: &str) -> bool {
    tag.starts_with(|c: char| c.is_ascii_lowercase())
        && tag
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
}

fn indent(depth: usize) -> String {
    "  ".repeat(depth)
}
