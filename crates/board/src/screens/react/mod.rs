//! A screen as a React component: a mechanical conversion of its HTML to a `.tsx` component and
//! its CSS ("Download React").
//!
//! The page is parsed as a browser parses it (html5ever), and its `<body>` written as the JSX the
//! component returns ([`jsx`]): attributes as React's props ([`attrs`], [`style`]), text with
//! HTML's whitespace made explicit, form values as `defaultValue`/`defaultChecked`. Style sheets
//! (`<style>` and local `<link rel="stylesheet">`, in document order) become the component's CSS
//! file, remote ones `@import`s at its top. Scripts and event-handler attributes can't run in a
//! component: they are left out, and their code is kept in a comment at the end of the file.
//! Local images and other files the page and its CSS refer to are listed to be copied beside
//! the component, and referred to there.

mod attrs;
mod jsx;
mod style;
#[cfg(test)]
mod tests;

use std::collections::HashMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use html5ever::Attribute;
use markup5ever_rcdom::{Handle, NodeData};

use super::inline::{HTML_NS, SVG_NS, attr, child_element, parse, rel_tokens, text_of};
use super::{INDEX, css, has_scheme, read, resolve};

/// A screen converted to React.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ReactExport {
    /// The component's name (`PricingPage`).
    pub component: String,
    /// The `.tsx` file.
    pub tsx: String,
    /// The `.css` file it imports.
    pub css: String,
    /// The local files the component and its CSS refer to: where each goes, from the folder the
    /// `.tsx` is written to (`PricingPage-assets/img/logo.png`), and the file in the screen's
    /// folder it is copied from.
    pub assets: Vec<(String, PathBuf)>,
    /// What the conversion left out or changed, one sentence each.
    pub notes: Vec<String>,
}

/// Screen folder `folder`'s `index.html` as React component `component`, its local assets
/// referred to under `assets_dir` (a folder name beside the `.tsx`) and the CSS file
/// `css_file` (the name the `.tsx` imports). An error only when there is no readable
/// `index.html`.
pub fn to_react(
    folder: &Path,
    component: &str,
    css_file: &str,
    assets_dir: &str,
) -> Result<ReactExport, String> {
    let page =
        read(folder, INDEX).map_err(|e| format!("couldn't read the screen's {INDEX}: {e}"))?;
    let html = String::from_utf8_lossy(&page);
    let mut export = convert(&html, component, css_file, assets_dir, &|rel| {
        read(folder, rel).ok()
    });
    for (_, source) in &mut export.assets {
        *source = folder.join(&*source);
    }
    Ok(export)
}

/// [`to_react`] for the page `html`, its files read by `read` (given a path from the screen's
/// folder; `None` for one that can't be read). An asset's source here is its path from the
/// screen's folder, which [`to_react`] puts the folder in front of. `component` must be a valid
/// identifier, as [`super::component_name`] makes one.
pub fn convert(
    html: &str,
    component: &str,
    css_file: &str,
    assets_dir: &str,
    read: &dyn Fn(&str) -> Option<Vec<u8>>,
) -> ReactExport {
    let dom = parse(html);
    let mut convert = Convert {
        read,
        assets_dir,
        exists: HashMap::new(),
        assets: Vec::new(),
        imports: Vec::new(),
        sheets: Vec::new(),
        remote: Vec::new(),
        left_out: Vec::new(),
        scripts: 0,
        important: Vec::new(),
        css_properties: false,
        custom: Vec::new(),
        notes: Vec::new(),
    };
    let root = child_element(&dom.document, "html");
    if let Some(root) = &root {
        convert.page_attrs(root);
        if let Some(head) = child_element(root, "head") {
            for child in head.children.borrow().iter() {
                // What stays in the JSX from the head is nothing; styles and scripts are taken.
                convert.stays(child);
            }
        }
    }
    let body = root.as_ref().and_then(|root| child_element(root, "body"));
    let mut jsx = String::new();
    convert.root(body.as_ref(), &mut jsx);
    convert.finish(component, css_file, &jsx)
}

/// What the conversion gathers as it walks the page.
struct Convert<'a> {
    read: &'a dyn Fn(&str) -> Option<Vec<u8>>,
    assets_dir: &'a str,
    /// Whether each local file referred to reads, by its path from the folder.
    exists: HashMap<String, bool>,
    assets: Vec<(String, PathBuf)>,
    /// The CSS file's `@import`s, which go first, and its sheets, in document order.
    imports: Vec<String>,
    sheets: Vec<String>,
    /// The remote style sheets linked.
    remote: Vec<String>,
    /// Scripts and event handlers, as the page had them, for the comment at the end.
    left_out: Vec<String>,
    scripts: usize,
    /// The style properties that lost their `!important`.
    important: Vec<String>,
    /// Whether a `style` object is cast to `CSSProperties`, which the file then imports.
    css_properties: bool,
    /// The custom elements' tags (`my-widget`), which the file declares for TypeScript.
    custom: Vec<String>,
    notes: Vec<String>,
}

impl Convert<'_> {
    /// Notes the `<html>` element's `lang` and `dir`, which a component can't set.
    fn page_attrs(&mut self, root: &Handle) {
        let NodeData::Element { attrs, .. } = &root.data else {
            return;
        };
        for name in ["lang", "dir"] {
            if let Some(value) = attr(&attrs.borrow(), name) {
                self.note(format!(
                    "The page's <html {name}=\"{value}\"> isn't part of a component: set it on your \
                     app's page."
                ));
            }
        }
    }

    /// Takes out of the page what isn't JSX, as the walk meets it: `<style>` and local
    /// `<link rel="stylesheet">` into the CSS, remote style sheets as `@import`s, scripts into the
    /// comment at the end; `<title>`, `<meta>`, `<base>` and `<noscript>` go. Whether `node`
    /// stays in the JSX.
    fn stays(&mut self, node: &Handle) -> bool {
        let NodeData::Element { name, attrs, .. } = &node.data else {
            return true;
        };
        let html = &*name.ns == HTML_NS;
        match &*name.local {
            "style" if html || &*name.ns == SVG_NS => {
                let media = attr(&attrs.borrow(), "media").map(str::to_string);
                self.add_sheet(&text_of(node), INDEX, media.as_deref());
                false
            }
            "script" if html || &*name.ns == SVG_NS => {
                self.script(node, &attrs.borrow());
                false
            }
            "link" if html => !self.link(&attrs.borrow()),
            "title" | "meta" | "base" if html => false,
            "noscript" if html => {
                self.note(
                    "<noscript> was left out: a React component renders only where scripts run."
                        .into(),
                );
                false
            }
            _ => true,
        }
    }

    /// Takes a `<link rel="stylesheet">` into the CSS: a local sheet read and added, a remote one
    /// as an `@import` (its `media` kept). Whether the link was a style sheet, which goes from
    /// the JSX either way.
    fn link(&mut self, attrs: &[Attribute]) -> bool {
        let rels = rel_tokens(attrs);
        if !rels.iter().any(|rel| rel == "stylesheet") {
            return false;
        }
        let href = attr(attrs, "href").unwrap_or_default().trim();
        // An alternate sheet is off until the reader picks it.
        if rels.iter().any(|rel| rel == "alternate") || href.is_empty() {
            return true;
        }
        let media = attr(attrs, "media")
            .map(str::trim)
            .filter(|media| !media.is_empty() && !media.eq_ignore_ascii_case("all"));
        if let Some(rel) = resolve(INDEX, href) {
            if let Some(bytes) = (self.read)(&rel) {
                self.add_sheet(&String::from_utf8_lossy(&bytes), &rel, media);
            }
        } else if has_scheme(href) || href.starts_with("//") {
            let media = media.map(|media| format!(" {media}")).unwrap_or_default();
            self.add_import(format!("@import {}{media};", css::url_token(href)));
            self.remote.push(href.to_string());
        }
        true
    }

    /// Adds style sheet `text`, the file `base`, to the CSS: its local `@import`s inlined, its
    /// `url(…)`s to local files pointed at the assets, inside `@media` when `media` is given.
    fn add_sheet(&mut self, text: &str, base: &str, media: Option<&str>) {
        let read = self.read;
        let load = |rel: &str| read(rel).map(|bytes| String::from_utf8_lossy(&bytes).into_owned());
        let flat = css::flatten(text, base, &load, &mut |sheet, href| {
            self.asset(sheet, href)
        });
        for import in flat.imports {
            self.add_import(import);
        }
        let body = match media.map(str::trim).filter(|media| !media.is_empty()) {
            Some(media) => css::wrap(media, &flat.body),
            None => flat.body,
        };
        let body = body.trim_matches(|c: char| c == '\n' || c == '\r');
        if !body.trim().is_empty() {
            self.sheets.push(body.to_string());
        }
    }

    fn add_import(&mut self, import: String) {
        if !self.imports.contains(&import) {
            self.imports.push(import);
        }
    }

    /// Keeps script `node` for the comment at the end: its tag and its code.
    fn script(&mut self, node: &Handle, attrs: &[Attribute]) {
        self.scripts += 1;
        let mut open = String::from("<script");
        for attribute in attrs {
            let _ = match attribute.value.is_empty() {
                true => write!(open, " {}", attribute.name.local),
                false => write!(open, " {}=\"{}\"", attribute.name.local, attribute.value),
            };
        }
        open.push('>');
        let code = text_of(node);
        let code = code.trim_matches(|c: char| c == '\n' || c == '\r');
        self.left_out.push(match code.trim().is_empty() {
            true => format!("{open}</script>"),
            false => format!("{open}\n{code}\n</script>"),
        });
    }

    /// `href`, a reference in the file `base`, as the path the component's files refer to it by,
    /// `./<assets_dir>/<path>` (its `#fragment` kept), when it names a file in the folder that
    /// reads; the file is listed among the assets once.
    fn asset(&mut self, base: &str, href: &str) -> Option<String> {
        let rel = resolve(base, href)?;
        let read = self.read;
        if !*self
            .exists
            .entry(rel.clone())
            .or_insert_with(|| read(&rel).is_some())
        {
            return None;
        }
        let target = format!("{}/{rel}", self.assets_dir);
        if !self.assets.iter().any(|(listed, _)| *listed == target) {
            self.assets.push((target, PathBuf::from(&rel)));
        }
        let fragment = href.find('#').map_or("", |at| &href[at..]);
        Some(format!(
            "./{}/{}{fragment}",
            encode_path(self.assets_dir),
            encode_path(&rel)
        ))
    }

    /// Adds `note`, on one line, unless it is there already.
    fn note(&mut self, note: String) {
        let note: String = note
            .chars()
            .map(|c| if c.is_control() { ' ' } else { c })
            .collect();
        if !self.notes.contains(&note) {
            self.notes.push(note);
        }
    }

    /// The export: the notes gathered, the `.tsx` around `jsx`, and the CSS.
    fn finish(mut self, component: &str, css_file: &str, jsx: &str) -> ReactExport {
        if !self.remote.is_empty() {
            let remote = self.remote.join(", ");
            self.note(format!(
                "Remote style sheets are @import-ed at the top of {css_file}: {remote}."
            ));
        }
        if self.scripts > 0 {
            let (count, them) = match self.scripts {
                1 => ("script was".to_string(), "its code is"),
                n => (format!("{n} scripts were"), "their code is"),
            };
            self.note(format!(
                "React components don't run <script> elements, so the page's {count} left out; \
                 {them} at the end of this file, to port by hand."
            ));
        }
        if !self.important.is_empty() {
            let properties = self.important.join(", ");
            self.note(format!(
                "React's style prop can't say !important, so it was dropped from: {properties}."
            ));
        }
        if !self.assets.is_empty() {
            let dir = self.assets_dir;
            self.note(format!(
                "Images and other files are in ./{dir}/ beside this file; <img src> paths are \
                 relative to the page, so move them to your app's public folder or import them."
            ));
        }
        let mut tsx = format!(
            "// Generated by Sawit from the screen \"{component}\": a mechanical conversion of its \
             HTML.\n"
        );
        for note in &self.notes {
            let _ = writeln!(tsx, "// {note}");
        }
        if self.css_properties {
            tsx.push_str("import type { CSSProperties } from \"react\";\n");
        }
        let _ = writeln!(tsx, "import {};\n", js_string(&format!("./{css_file}")));
        let _ = writeln!(tsx, "export default function {component}() {{");
        tsx.push_str("  return (\n");
        tsx.push_str(jsx);
        tsx.push_str("  );\n}\n");
        if !self.custom.is_empty() {
            tsx.push_str(
                "\n// The page's custom elements, for TypeScript; they render as written, and \
                 work once their definitions load.\ndeclare module \"react\" {\n  namespace JSX \
                 {\n    interface IntrinsicElements {\n",
            );
            for tag in &self.custom {
                let _ = writeln!(tsx, "      {}: Record<string, unknown>;", js_string(tag));
            }
            tsx.push_str("    }\n  }\n}\n");
        }
        if !self.left_out.is_empty() {
            tsx.push_str("\n/*\nLeft out of the component, to port by hand:\n");
            for entry in &self.left_out {
                let _ = write!(tsx, "\n{}\n", entry.replace("*/", "*\\/"));
            }
            tsx.push_str("*/\n");
        }
        let mut css = self.imports.join("\n");
        if !css.is_empty() && !self.sheets.is_empty() {
            css.push_str("\n\n");
        }
        css.push_str(&self.sheets.join("\n\n"));
        if !css.is_empty() {
            css.push('\n');
        }
        ReactExport {
            component: component.to_string(),
            tsx,
            css,
            assets: self.assets,
            notes: self.notes,
        }
    }
}

/// `text` as a JavaScript string literal, in double quotes.
fn js_string(text: &str) -> String {
    serde_json::to_string(text).unwrap_or_else(|_| "\"\"".into())
}

/// `path` as a URL path: each byte other than letters, digits, `/` and `-._~!$&'()*+;=:@`
/// percent-encoded, so a space or a comma can't split a `srcset`.
fn encode_path(path: &str) -> String {
    let mut out = String::with_capacity(path.len());
    for byte in path.bytes() {
        match byte {
            b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'/' => out.push(byte as char),
            b'-' | b'.' | b'_' | b'~' | b'!' | b'$' | b'&' | b'\'' | b'(' | b')' | b'*' | b'+'
            | b';' | b'=' | b':' | b'@' => out.push(byte as char),
            _ => {
                let _ = write!(out, "%{byte:02X}");
            }
        }
    }
    out
}
