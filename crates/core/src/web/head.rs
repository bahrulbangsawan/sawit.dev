//! A page's HTML read with `html5ever` into what the favicons and the SEO scan look at: the
//! title, `<meta>` and `<link>` tags, JSON-LD blocks, the headings, images, links and forms, and
//! how much text there is. It reads the HTML the server sent, or the page as rendered (the DOM
//! the page sends back as HTML), the same way.

use std::rc::Rc;

use html5ever::tendril::TendrilSink;
use html5ever::{ParseOpts, parse_document};
use markup5ever_rcdom::{Handle, NodeData, RcDom};

use super::Url;

/// A `<meta>` tag.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Meta {
    /// `name`, `property` or `http-equiv`, lower case.
    pub key: String,
    pub content: String,
}

/// A `<link>` tag, its `href` made absolute.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Link {
    /// The `rel` words, lower case.
    pub rel: Vec<String>,
    pub href: String,
    pub sizes: Option<String>,
    pub mime: Option<String>,
    pub hreflang: Option<String>,
    pub media: Option<String>,
}

impl Link {
    pub fn has_rel(&self, rel: &str) -> bool {
        self.rel.iter().any(|r| r == rel)
    }
}

/// An `<h1>`…`<h6>`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Heading {
    pub level: u8,
    pub text: String,
}

/// An `<img>`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Image {
    pub src: String,
    /// `None` when the attribute is missing; `Some("")` marks a decorative image.
    pub alt: Option<String>,
}

/// An `<a href>`, made absolute.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Anchor {
    pub href: String,
    pub text: String,
    pub rel: Vec<String>,
}

/// A form control: its kind and whether something names it (a `<label>`, `aria-label`,
/// `aria-labelledby`, `title`, or a button's own text).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Control {
    pub kind: String,
    pub named: bool,
}

/// What the page's HTML says.
#[derive(Clone, Debug, Default)]
pub struct Head {
    /// The address the HTML came from, `<base href>` applied.
    pub base: String,
    pub lang: Option<String>,
    pub title: Option<String>,
    pub metas: Vec<Meta>,
    pub links: Vec<Link>,
    /// The text of each `<script type="application/ld+json">`.
    pub json_ld: Vec<String>,
    pub headings: Vec<Heading>,
    pub images: Vec<Image>,
    pub anchors: Vec<Anchor>,
    pub controls: Vec<Control>,
    /// Elements with a click role but no native control (`div onclick`, `role=button` on a
    /// `div`), which agents can't always find.
    pub fake_buttons: usize,
    /// Visible text, whitespace collapsed (script and style left out).
    pub text: String,
    /// `<script>` tags with a `src`, and inline ones.
    pub scripts: usize,
    /// Forms with a `toolname` or `tooldescription` attribute (declared tools for agents).
    pub tool_forms: usize,
    /// The size of the HTML, in bytes.
    pub bytes: usize,
    /// Landmarks: `main`, `nav`, `header`, `footer`, `aside` and their ARIA roles, by name.
    pub landmarks: Vec<String>,
    /// Inline script text (for declared page tools and the like), up to 256 KB.
    pub inline_scripts: String,
}

impl Head {
    /// The first `<meta>` whose key is `key` (lower case), its content trimmed.
    pub fn meta(&self, key: &str) -> Option<&str> {
        self.metas
            .iter()
            .find(|m| m.key == key)
            .map(|m| m.content.trim())
    }

    /// Every `<meta>` whose key is `key`.
    pub fn metas_named<'a>(&'a self, key: &'a str) -> impl Iterator<Item = &'a str> + 'a {
        self.metas
            .iter()
            .filter(move |m| m.key == key)
            .map(|m| m.content.trim())
    }

    /// The links with `rel` among their words.
    pub fn links_rel<'a>(&'a self, rel: &'a str) -> impl Iterator<Item = &'a Link> + 'a {
        self.links.iter().filter(move |l| l.has_rel(rel))
    }

    /// The canonical address, if any.
    pub fn canonical(&self) -> Option<&str> {
        self.links_rel("canonical").next().map(|l| l.href.as_str())
    }
}

/// Reads `html`, which came from `url`.
pub fn parse(html: &str, url: &str) -> Head {
    let dom = parse_document(RcDom::default(), ParseOpts::default())
        .from_utf8()
        .read_from(&mut std::io::Cursor::new(html.as_bytes()))
        .unwrap_or_default();
    let mut head = Head {
        base: url.to_string(),
        bytes: html.len(),
        ..Default::default()
    };
    let mut walk = Walk {
        head: &mut head,
        base: Url::parse(url).ok(),
        labelled_ids: Vec::new(),
        controls_ids: Vec::new(),
        text: String::new(),
    };
    walk.node(&dom.document, false);
    // `<label for=…>` named controls by id: settled once every label is known.
    let Walk {
        labelled_ids,
        controls_ids,
        text,
        ..
    } = walk;
    for (i, id) in controls_ids {
        if let Some(id) = id
            && labelled_ids.contains(&id)
            && let Some(c) = head.controls.get_mut(i)
        {
            c.named = true;
        }
    }
    head.text = collapse(&text);
    head
}

struct Walk<'a> {
    head: &'a mut Head,
    base: Option<Url>,
    /// Ids that a `<label for>` names.
    labelled_ids: Vec<String>,
    /// Each control's place in `head.controls` and its id.
    controls_ids: Vec<(usize, Option<String>)>,
    text: String,
}

fn attrs(handle: &Handle) -> Vec<(String, String)> {
    match &handle.data {
        NodeData::Element { attrs, .. } => attrs
            .borrow()
            .iter()
            .map(|a| (a.name.local.to_string(), a.value.to_string()))
            .collect(),
        _ => Vec::new(),
    }
}

fn get<'a>(attrs: &'a [(String, String)], name: &str) -> Option<&'a str> {
    attrs
        .iter()
        .find(|(n, _)| n.eq_ignore_ascii_case(name))
        .map(|(_, v)| v.as_str())
}

/// The text inside `handle`, scripts and styles left out.
fn text_of(handle: &Handle) -> String {
    let mut out = String::new();
    fn go(h: &Handle, out: &mut String) {
        match &h.data {
            NodeData::Text { contents } => out.push_str(&contents.borrow()),
            NodeData::Element { name, attrs, .. } => {
                let tag = name.local.as_ref();
                if matches!(tag, "script" | "style" | "template" | "noscript") {
                    return;
                }
                if tag == "img"
                    && let Some(alt) = attrs
                        .borrow()
                        .iter()
                        .find(|a| a.name.local.as_ref() == "alt")
                {
                    out.push_str(&alt.value);
                }
                for c in h.children.borrow().iter() {
                    go(c, out);
                }
            }
            _ => {
                for c in h.children.borrow().iter() {
                    go(c, out);
                }
            }
        }
    }
    go(handle, &mut out);
    collapse(&out)
}

fn collapse(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

impl Walk<'_> {
    fn absolute(&self, href: &str) -> String {
        let href = href.trim();
        match &self.base {
            Some(base) => base
                .join(href)
                .map(|u| u.to_string())
                .unwrap_or_else(|_| href.to_string()),
            None => href.to_string(),
        }
    }

    fn node(&mut self, handle: &Handle, hidden: bool) {
        match &handle.data {
            NodeData::Document => self.children(handle, hidden),
            NodeData::Text { contents } => {
                if !hidden {
                    self.text.push_str(&contents.borrow());
                    self.text.push(' ');
                }
            }
            NodeData::Element { name, .. } => {
                let tag = name.local.as_ref().to_ascii_lowercase();
                self.element(&tag, handle, hidden);
            }
            _ => {}
        }
    }

    fn children(&mut self, handle: &Handle, hidden: bool) {
        let children: Vec<Rc<_>> = handle.children.borrow().iter().cloned().collect();
        for c in &children {
            self.node(c, hidden);
        }
    }

    fn element(&mut self, tag: &str, handle: &Handle, hidden: bool) {
        let a = attrs(handle);
        match tag {
            "html" => {
                self.head.lang = get(&a, "lang").map(|l| l.trim().to_string());
            }
            "base" => {
                if let Some(href) = get(&a, "href") {
                    let abs = self.absolute(href);
                    self.base = Url::parse(&abs).ok().or(self.base.take());
                    self.head.base = abs;
                }
            }
            "title" => {
                if self.head.title.is_none() {
                    self.head.title = Some(text_of(handle));
                }
                return;
            }
            "meta" => {
                let key = get(&a, "name")
                    .or_else(|| get(&a, "property"))
                    .or_else(|| get(&a, "http-equiv"))
                    .or_else(|| get(&a, "itemprop"));
                if let (Some(key), Some(content)) = (key, get(&a, "content")) {
                    self.head.metas.push(Meta {
                        key: key.trim().to_ascii_lowercase(),
                        content: content.to_string(),
                    });
                } else if let Some(charset) = get(&a, "charset") {
                    self.head.metas.push(Meta {
                        key: "charset".into(),
                        content: charset.to_string(),
                    });
                }
            }
            "link" => {
                if let Some(href) = get(&a, "href") {
                    self.head.links.push(Link {
                        rel: get(&a, "rel")
                            .unwrap_or_default()
                            .split_whitespace()
                            .map(|r| r.to_ascii_lowercase())
                            .collect(),
                        href: self.absolute(href),
                        sizes: get(&a, "sizes").map(String::from),
                        mime: get(&a, "type").map(|t| t.to_ascii_lowercase()),
                        hreflang: get(&a, "hreflang").map(String::from),
                        media: get(&a, "media").map(String::from),
                    });
                }
            }
            "script" => {
                self.head.scripts += 1;
                if get(&a, "src").is_none() && self.head.inline_scripts.len() < 256 * 1024 {
                    for c in handle.children.borrow().iter() {
                        if let NodeData::Text { contents } = &c.data {
                            self.head.inline_scripts.push_str(&contents.borrow());
                            self.head.inline_scripts.push('\n');
                        }
                    }
                }
                if get(&a, "type").is_some_and(|t| t.eq_ignore_ascii_case("application/ld+json")) {
                    let mut text = String::new();
                    for c in handle.children.borrow().iter() {
                        if let NodeData::Text { contents } = &c.data {
                            text.push_str(&contents.borrow());
                        }
                    }
                    self.head.json_ld.push(text);
                }
                return;
            }
            "style" | "template" | "noscript" => return,
            "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                self.head.headings.push(Heading {
                    level: tag.as_bytes()[1] - b'0',
                    text: text_of(handle),
                });
            }
            "img" => {
                let src = get(&a, "src")
                    .or_else(|| get(&a, "data-src"))
                    .unwrap_or_default();
                self.head.images.push(Image {
                    src: self.absolute(src),
                    alt: get(&a, "alt").map(|t| t.trim().to_string()),
                });
            }
            "a" => {
                if let Some(href) = get(&a, "href") {
                    self.head.anchors.push(Anchor {
                        href: self.absolute(href),
                        text: text_of(handle),
                        rel: get(&a, "rel")
                            .unwrap_or_default()
                            .split_whitespace()
                            .map(|r| r.to_ascii_lowercase())
                            .collect(),
                    });
                }
            }
            "label" => {
                if let Some(id) = get(&a, "for") {
                    self.labelled_ids.push(id.to_string());
                }
                // A control inside its label is named by it.
                let before = self.head.controls.len();
                self.children(handle, hidden);
                for c in &mut self.head.controls[before..] {
                    c.named = true;
                }
                return;
            }
            "main" | "nav" | "header" | "footer" | "aside" => {
                self.head.landmarks.push(tag.to_string());
            }
            "form" => {
                if get(&a, "toolname").is_some() || get(&a, "tooldescription").is_some() {
                    self.head.tool_forms += 1;
                }
            }
            "input" | "select" | "textarea" | "button" => {
                let kind = if tag == "input" {
                    get(&a, "type").unwrap_or("text").to_ascii_lowercase()
                } else {
                    tag.to_string()
                };
                if kind != "hidden" {
                    let named = get(&a, "aria-label").is_some_and(|l| !l.trim().is_empty())
                        || get(&a, "aria-labelledby").is_some()
                        || get(&a, "title").is_some_and(|l| !l.trim().is_empty())
                        || (matches!(kind.as_str(), "submit" | "button" | "reset")
                            && get(&a, "value").is_some())
                        || (tag == "button" && !text_of(handle).is_empty())
                        || (kind == "image" && get(&a, "alt").is_some());
                    self.controls_ids
                        .push((self.head.controls.len(), get(&a, "id").map(String::from)));
                    self.head.controls.push(Control { kind, named });
                }
            }
            "div" | "span" | "li" | "td" => {
                let role = get(&a, "role").unwrap_or_default();
                if get(&a, "onclick").is_some() || role.eq_ignore_ascii_case("button") {
                    self.head.fake_buttons += 1;
                }
            }
            _ => {}
        }
        if let Some(role) = get(&a, "role") {
            let role = role.to_ascii_lowercase();
            if matches!(
                role.as_str(),
                "main" | "navigation" | "banner" | "contentinfo" | "complementary" | "search"
            ) {
                self.head.landmarks.push(role);
            }
        }
        let hidden =
            hidden || get(&a, "hidden").is_some() || get(&a, "aria-hidden") == Some("true");
        self.children(handle, hidden);
    }
}
