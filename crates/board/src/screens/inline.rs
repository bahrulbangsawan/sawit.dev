//! A screen as one HTML file, its CSS and images inlined ("Download HTML").
//!
//! The page is parsed as a browser parses it (html5ever) and written back with its local files in
//! it: style sheets as `<style>` (their local `@import`s inlined, their `url(…)`s resolved from
//! the sheet's own file), scripts inline, and images, icons, fonts and media as `data:` URLs.
//! What isn't a file in the screen's folder (a remote URL, `data:`, a `#fragment`, a missing
//! file, a path that leaves the folder) stays as written. The doctype is written as the page had
//! it, and a page with none gets none, so a page in quirks mode stays in it.

#[cfg(test)]
mod tests;

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::Path;
use std::rc::Rc;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use html5ever::tendril::{StrTendril, TendrilSink};
use html5ever::{Attribute, LocalName, Namespace, QualName};
use markup5ever_rcdom::{Handle, Node, NodeData, RcDom, SerializableHandle};

use super::{INDEX, css, mime, read, resolve};

/// The namespaces html5ever puts elements and attributes in.
pub(super) const HTML_NS: &str = "http://www.w3.org/1999/xhtml";
pub(super) const SVG_NS: &str = "http://www.w3.org/2000/svg";
pub(super) const XLINK_NS: &str = "http://www.w3.org/1999/xlink";

/// Screen folder `folder`'s `index.html` as one file: local style sheets as `<style>`, local
/// scripts inline, local images, fonts and media as `data:` URLs. What isn't in the folder
/// (remote URLs, missing files) stays as it was. An error only when there is no readable
/// `index.html`.
pub fn single_file(folder: &Path) -> Result<String, String> {
    let page =
        read(folder, INDEX).map_err(|e| format!("couldn't read the screen's {INDEX}: {e}"))?;
    let html = String::from_utf8_lossy(&page);
    Ok(inline_html(&html, &|rel| read(folder, rel).ok()))
}

/// [`single_file`] for the page `html`, its files read by `read` (given a path from the screen's
/// folder, as [`resolve`] gives it; `None` for a file that can't be read).
///
/// A classic script that was `defer` or `async` runs once the page is parsed, so inlined it moves
/// to the end of `<body>`, where an inline script runs at that point too.
pub fn inline_html(html: &str, read: &dyn Fn(&str) -> Option<Vec<u8>>) -> String {
    let dom = parse(html);
    let body = child_element(&dom.document, "html").and_then(|h| child_element(&h, "body"));
    let mut inliner = Inliner {
        read,
        data: HashMap::new(),
        deferred: body.as_ref().map(|_| Vec::new()),
    };
    inliner.walk(&dom.document);
    if let (Some(body), Some(deferred)) = (body, inliner.deferred) {
        for script in deferred {
            script.parent.set(Some(Rc::downgrade(&body)));
            body.children.borrow_mut().push(script);
        }
    }
    let mut out = Vec::new();
    let handle: SerializableHandle = dom.document.clone().into();
    let _ = html5ever::serialize(&mut out, &handle, Default::default());
    String::from_utf8_lossy(&out).into_owned()
}

/// What inlining keeps while it walks the page.
struct Inliner<'a> {
    read: &'a dyn Fn(&str) -> Option<Vec<u8>>,
    /// Each file's `data:` URL, by its path from the folder; `None` for one that can't be read.
    data: HashMap<String, Option<Rc<str>>>,
    /// Scripts that were `defer` or `async`, inlined, for the end of `<body>`; `None` for a page
    /// with no `<body>` (a frameset), whose scripts stay where they are.
    deferred: Option<Vec<Handle>>,
}

impl Inliner<'_> {
    /// Inlines what `parent`'s children (and theirs) refer to.
    fn walk(&mut self, parent: &Handle) {
        let children: Vec<Handle> = parent.children.borrow().clone();
        let mut swaps: Vec<(usize, Option<Handle>)> = Vec::new();
        for (index, child) in children.iter().enumerate() {
            let NodeData::Element {
                name,
                attrs,
                template_contents,
                ..
            } = &child.data
            else {
                continue;
            };
            let html = &*name.ns == HTML_NS;
            match &*name.local {
                "link" if html => {
                    if let Some(style) = self.stylesheet(&attrs.borrow()) {
                        swaps.push((index, Some(style)));
                        continue;
                    }
                }
                "style" if html || &*name.ns == SVG_NS => {
                    let text = self.sheet(&text_of(child), INDEX);
                    set_text(child, &escape_end_tag(&text, "style"));
                    continue;
                }
                "script" if html => {
                    if self.script(child)
                        && let Some(deferred) = &mut self.deferred
                    {
                        swaps.push((index, None));
                        deferred.push(child.clone());
                    }
                    continue;
                }
                _ => {}
            }
            self.attributes(name, &mut attrs.borrow_mut());
            self.walk(child);
            // A template's content isn't among its children, where the serializer looks.
            if let Some(content) = template_contents.borrow().as_ref() {
                self.walk(content);
                let moved = std::mem::take(&mut *content.children.borrow_mut());
                for node in &moved {
                    node.parent.set(Some(Rc::downgrade(child)));
                }
                child.children.borrow_mut().extend(moved);
            }
        }
        let mut kids = parent.children.borrow_mut();
        for (index, swap) in swaps.into_iter().rev() {
            match swap {
                Some(node) => {
                    node.parent.set(Some(Rc::downgrade(parent)));
                    kids[index] = node;
                }
                None => {
                    kids.remove(index);
                }
            }
        }
    }

    /// The `<style>` that takes the place of a `<link rel="stylesheet">` with these attributes,
    /// when it names a sheet in the folder that reads (its `media` kept).
    fn stylesheet(&mut self, attrs: &[Attribute]) -> Option<Handle> {
        let rels = rel_tokens(attrs);
        if !rels.iter().any(|r| r == "stylesheet") || rels.iter().any(|r| r == "alternate") {
            return None;
        }
        let rel = resolve(INDEX, attr(attrs, "href")?)?;
        let text = String::from_utf8_lossy(&(self.read)(&rel)?).into_owned();
        let sheet = self.sheet(&text, &rel);
        let media = attrs
            .iter()
            .filter(|a| a.name.ns.is_empty() && &*a.name.local == "media");
        let style = element("style", media.cloned().collect());
        set_text(&style, &escape_end_tag(&sheet, "style"));
        Some(style)
    }

    /// Style sheet `text`, the file `base`, with its local imports and references inlined.
    fn sheet(&mut self, text: &str, base: &str) -> String {
        let read = self.read;
        let load = |rel: &str| read(rel).map(|bytes| String::from_utf8_lossy(&bytes).into_owned());
        css::flatten(text, base, &load, &mut |sheet, href| {
            self.data_for(sheet, href)
        })
        .text()
    }

    /// Puts a local script's code in `script` in place of its `src`. Whether it is a classic
    /// script that was `defer` or `async` on a page with a `<body>`, which then goes to its end
    /// (and loses `defer` and `async`, which mean nothing inline).
    fn script(&mut self, script: &Handle) -> bool {
        let NodeData::Element { attrs, .. } = &script.data else {
            return false;
        };
        let mut attrs = attrs.borrow_mut();
        let Some(rel) = attr(&attrs, "src").and_then(|src| resolve(INDEX, src)) else {
            return false;
        };
        let Some(code) = (self.read)(&rel) else {
            return false;
        };
        let classic = attr(&attrs, "type").is_none_or(|t| {
            let t = t.trim().to_ascii_lowercase();
            t.is_empty() || t.contains("javascript") || t.contains("ecmascript")
        });
        let deferred = classic
            && self.deferred.is_some()
            && (attr(&attrs, "defer").is_some() || attr(&attrs, "async").is_some());
        attrs.retain(|a| {
            let gone = match deferred {
                true => &["src", "integrity", "defer", "async"][..],
                false => &["src", "integrity"][..],
            };
            !(a.name.ns.is_empty() && gone.contains(&&*a.name.local))
        });
        drop(attrs);
        set_text(
            script,
            &escape_end_tag(&String::from_utf8_lossy(&code), "script"),
        );
        deferred
    }

    /// Rewrites the attributes of element `name` that refer to local files: its `style`'s
    /// `url(…)`s, and the URLs [`asset_attr`] names.
    fn attributes(&mut self, name: &QualName, attrs: &mut [Attribute]) {
        let kinds: Vec<Option<AssetAttr>> = attrs
            .iter()
            .map(|a| asset_attr(name, &a.name, attrs))
            .collect();
        for (attribute, kind) in attrs.iter_mut().zip(kinds) {
            let value = attribute.value.to_string();
            let style = attribute.name.ns.is_empty() && &*attribute.name.local == "style";
            let new = match kind {
                Some(AssetAttr::Url) => self.data_for(INDEX, &value),
                Some(AssetAttr::Srcset) => rewrite_srcset(&value, |url| self.data_for(INDEX, url)),
                None if style => {
                    let new = css::rewrite_urls(&value, |href| self.data_for(INDEX, href));
                    (new != value).then_some(new)
                }
                None => None,
            };
            if let Some(new) = new {
                attribute.value = StrTendril::from(new);
            }
        }
    }

    /// `href`, a reference in the file `base`, as a `data:` URL when it names a file in the
    /// folder that reads; its `#fragment` kept.
    fn data_for(&mut self, base: &str, href: &str) -> Option<String> {
        let rel = resolve(base, href)?;
        let read = self.read;
        let data = self.data.entry(rel).or_insert_with_key(|rel| {
            let bytes = read(rel)?;
            let kind = mime(rel).replace(' ', "");
            Some(format!("data:{kind};base64,{}", STANDARD.encode(bytes)).into())
        });
        let fragment = href.find('#').map_or("", |at| &href[at..]);
        data.as_ref().map(|data| format!("{data}{fragment}"))
    }
}

/// How an attribute refers to a file the page shows: one URL, or a `srcset` list of them.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum AssetAttr {
    Url,
    Srcset,
}

/// Whether attribute `attr` of element `element` (whose attributes are `attrs`) refers to a file
/// the page loads to show itself: `<img src|srcset>`, `<source src|srcset>`, `<video src|poster>`,
/// `<audio src>`, `<track src>`, `<embed src>`, `<object data>`, `<input type=image src>`, an
/// icon or preload `<link href>`, and SVG `<image>`, `<use>` and `<feImage>` `href` (or
/// `xlink:href`). Links to pages (`<a href>`) aren't.
pub fn asset_attr(element: &QualName, attr: &QualName, attrs: &[Attribute]) -> Option<AssetAttr> {
    let plain = attr.ns.is_empty();
    let (tag, name) = (&*element.local, &*attr.local);
    match &*element.ns {
        HTML_NS if plain => match (tag, name) {
            ("img" | "source", "srcset") => Some(AssetAttr::Srcset),
            ("img" | "source" | "audio" | "video" | "track" | "embed", "src")
            | ("video", "poster")
            | ("object", "data") => Some(AssetAttr::Url),
            ("input", "src") => attr_is(attrs, "type", "image").then_some(AssetAttr::Url),
            ("link", "href" | "imagesrcset") => {
                let loads = rel_tokens(attrs).iter().any(|rel| {
                    rel.contains("icon")
                        || matches!(&**rel, "preload" | "prefetch" | "modulepreload")
                });
                match (loads, name) {
                    (false, _) => None,
                    (true, "href") => Some(AssetAttr::Url),
                    (true, _) => Some(AssetAttr::Srcset),
                }
            }
            _ => None,
        },
        SVG_NS if plain || &*attr.ns == XLINK_NS => match (tag, name) {
            ("image" | "use" | "feImage", "href") => Some(AssetAttr::Url),
            _ => None,
        },
        _ => None,
    }
}

/// The candidates of a `srcset`, each URL with its descriptors (`2x`, `640w`; empty when it has
/// none), split as the HTML standard splits them: a URL runs to whitespace, so one may hold a
/// comma (`data:`), and one that ends in commas ends its candidate.
pub fn srcset(value: &str) -> Vec<(String, String)> {
    let b = value.as_bytes();
    let space = |c: u8| matches!(c, b' ' | b'\t' | b'\n' | b'\r' | b'\x0c');
    let mut found = Vec::new();
    let mut i = 0;
    loop {
        while i < b.len() && (space(b[i]) || b[i] == b',') {
            i += 1;
        }
        if i >= b.len() {
            return found;
        }
        let start = i;
        while i < b.len() && !space(b[i]) {
            i += 1;
        }
        let mut url = &value[start..i];
        let mut descriptors = "";
        if url.ends_with(',') {
            url = url.trim_end_matches(',');
        } else {
            let from = i;
            let mut depth = 0usize;
            while i < b.len() && !(b[i] == b',' && depth == 0) {
                match b[i] {
                    b'(' => depth += 1,
                    b')' => depth = depth.saturating_sub(1),
                    _ => {}
                }
                i += 1;
            }
            descriptors = value[from..i].trim();
        }
        if !url.is_empty() {
            found.push((url.to_string(), descriptors.to_string()));
        }
    }
}

/// `srcset` with each URL `f` gives a new one for rewritten, `None` when it gives none.
pub fn rewrite_srcset(value: &str, mut f: impl FnMut(&str) -> Option<String>) -> Option<String> {
    let mut changed = false;
    let candidates: Vec<String> = srcset(value)
        .into_iter()
        .map(|(url, descriptors)| {
            let url = f(&url).inspect(|_| changed = true).unwrap_or(url);
            match descriptors.is_empty() {
                true => url,
                false => format!("{url} {descriptors}"),
            }
        })
        .collect();
    changed.then(|| candidates.join(", "))
}

/// `html` parsed as a browser parses a page.
pub(super) fn parse(html: &str) -> RcDom {
    html5ever::parse_document(RcDom::default(), Default::default()).one(html)
}

/// The value of `attrs`' attribute `name` (one in no namespace).
pub(super) fn attr<'a>(attrs: &'a [Attribute], name: &str) -> Option<&'a str> {
    let found = attrs
        .iter()
        .find(|a| a.name.ns.is_empty() && &*a.name.local == name);
    found.map(|a| &*a.value)
}

/// Whether `attrs`' attribute `name` is `value`, in any case.
fn attr_is(attrs: &[Attribute], name: &str, value: &str) -> bool {
    attr(attrs, name).is_some_and(|v| v.trim().eq_ignore_ascii_case(value))
}

/// The tokens of a `rel` attribute, lowercased.
pub(super) fn rel_tokens(attrs: &[Attribute]) -> Vec<String> {
    let rel = attr(attrs, "rel").unwrap_or_default().to_ascii_lowercase();
    rel.split_ascii_whitespace().map(str::to_string).collect()
}

/// `node`'s first child element named `name` (an HTML one).
pub(super) fn child_element(node: &Handle, name: &str) -> Option<Handle> {
    let children = node.children.borrow();
    let found = children.iter().find(|child| match &child.data {
        NodeData::Element { name: n, .. } => &*n.ns == HTML_NS && &*n.local == name,
        _ => false,
    });
    found.cloned()
}

/// The text of `node`'s text children, joined.
pub(super) fn text_of(node: &Handle) -> String {
    let mut text = String::new();
    for child in node.children.borrow().iter() {
        if let NodeData::Text { contents } = &child.data {
            text.push_str(&contents.borrow());
        }
    }
    text
}

/// A new HTML element `name` with `attrs`.
fn element(name: &str, attrs: Vec<Attribute>) -> Handle {
    Node::new(NodeData::Element {
        name: QualName::new(None, Namespace::from(HTML_NS), LocalName::from(name)),
        attrs: RefCell::new(attrs),
        template_contents: RefCell::new(None),
        mathml_annotation_xml_integration_point: false,
    })
}

/// Makes `text` the one child of `node`.
fn set_text(node: &Handle, text: &str) {
    let child = Node::new(NodeData::Text {
        contents: RefCell::new(StrTendril::from(text)),
    });
    child.parent.set(Some(Rc::downgrade(node)));
    *node.children.borrow_mut() = vec![child];
}

/// `text`, the content of a `<tag>`, with each `</tag` (in any case) written `<\/tag`, so the
/// element ends where it should: `\/` is still `/` in JavaScript strings and CSS.
pub(super) fn escape_end_tag(text: &str, tag: &str) -> String {
    let lower = text.to_ascii_lowercase();
    let needle = format!("</{tag}");
    let mut out = String::with_capacity(text.len());
    let mut last = 0;
    for (at, _) in lower.match_indices(&needle) {
        out.push_str(&text[last..at]);
        out.push_str("<\\/");
        last = at + 2;
    }
    out.push_str(&text[last..]);
    out
}
