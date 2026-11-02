//! A style sheet's local `@import`s inlined, each sheet's `url(…)`s resolved from its own file.

use std::fmt::Write as _;

use super::{Kind, function_close, references, token};
use crate::screens::resolve;

/// The deepest a chain of local `@import`s is followed: a sheet that the page's sheet imports
/// is one deep.
const DEEPEST_IMPORT: usize = 4;

/// A style sheet with its local `@import`s inlined ([`flatten`]).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Flat {
    /// The `@import` rules it keeps, as written: remote ones, and local ones that don't read.
    /// They go first, since a browser ignores an `@import` that follows another rule.
    pub imports: Vec<String>,
    /// The rest of the sheet, the imported sheets in place of their rules.
    pub body: String,
}

impl Flat {
    /// The sheet as one text: its `@import`s, then the rest.
    pub fn text(&self) -> String {
        match self.imports.is_empty() {
            true => self.body.clone(),
            false => format!("{}\n{}", self.imports.join("\n"), self.body),
        }
    }
}

/// Style sheet `css`, the file `base` of a screen's folder (`index.html` for a `<style>`), with
/// its local `@import`s inlined: each imported sheet, read by `load` (given a path from the
/// folder), takes its rule's place, wrapped in the `@layer`, `@supports` and `@media` blocks its
/// rule's conditions name, its own imports inlined too, at most a few sheets deep. An import of
/// a sheet already being inlined (a cycle) is left out. Every `url(…)` goes through
/// `url(sheet, href)`, `sheet` the file it is in, so a reference resolves from its own sheet;
/// what it gives `None` for stays as written. The `@import`s kept (remote, or not readable) are
/// moved to [`Flat::imports`], those of imported sheets too; they lose the conditions of the rule
/// that imported their sheet. A leading `@charset` goes: a sheet inlined into another has none.
pub fn flatten(
    css: &str,
    base: &str,
    load: &dyn Fn(&str) -> Option<String>,
    url: &mut dyn FnMut(&str, &str) -> Option<String>,
) -> Flat {
    let mut chain = vec![base.to_string()];
    flatten_in(css, base, load, url, &mut chain)
}

fn flatten_in(
    css: &str,
    base: &str,
    load: &dyn Fn(&str) -> Option<String>,
    url: &mut dyn FnMut(&str, &str) -> Option<String>,
    chain: &mut Vec<String>,
) -> Flat {
    let css = without_charset(css);
    let mut flat = Flat::default();
    let mut last = 0;
    for reference in references(css) {
        flat.body.push_str(&css[last..reference.span.start]);
        last = reference.span.end;
        let written = &css[reference.span.clone()];
        let Kind::Import { conditions } = &reference.kind else {
            match url(base, &reference.href) {
                Some(new) => flat.body.push_str(&token(&reference.kind, &new)),
                None => flat.body.push_str(written),
            }
            continue;
        };
        let local = resolve(base, &reference.href);
        let sheet = local
            .as_ref()
            .filter(|rel| !chain.contains(rel) && chain.len() <= DEEPEST_IMPORT)
            .and_then(|rel| Some((rel.clone(), load(rel)?)));
        match sheet {
            Some((rel, text)) => {
                chain.push(rel.clone());
                let inner = flatten_in(&text, &rel, load, url, chain);
                chain.pop();
                flat.imports.extend(inner.imports);
                flat.body.push_str(&wrap(conditions, &inner.body));
            }
            // A cycle: the sheet is already in.
            None if local.as_ref().is_some_and(|rel| chain.contains(rel)) => {}
            None => {
                let rule = written.trim();
                flat.imports.push(match rule.ends_with(';') {
                    true => rule.to_string(),
                    false => format!("{rule};"),
                });
            }
        }
    }
    flat.body.push_str(&css[last..]);
    flat
}

/// `body` inside the blocks an `@import`'s `conditions` name: `layer(name)` (or `layer`) as
/// `@layer`, `supports(…)` as `@supports`, and the media query list that follows as `@media`.
pub fn wrap(conditions: &str, body: &str) -> String {
    let mut rest = conditions.trim();
    let mut blocks: Vec<String> = Vec::new();
    if let Some(after) = strip_prefix_ci(rest, "layer") {
        if let Some(inner) = after.strip_prefix('(') {
            if let Some(close) = close_paren(inner) {
                blocks.push(format!("@layer {}", inner[..close].trim()));
                rest = inner[close + 1..].trim_start();
            }
        } else if after.is_empty() || after.starts_with(|c: char| c.is_ascii_whitespace()) {
            blocks.push("@layer".into());
            rest = after.trim_start();
        }
    }
    if let Some(inner) = strip_prefix_ci(rest, "supports(")
        && let Some(close) = close_paren(inner)
    {
        let condition = inner[..close].trim();
        // A declaration (`display: grid`) needs its parentheses; a condition has its own.
        let declaration = match (condition.find(':'), condition.find('(')) {
            (Some(colon), Some(paren)) => colon < paren,
            (Some(_), None) => true,
            _ => false,
        };
        blocks.push(match declaration {
            true => format!("@supports ({condition})"),
            false => format!("@supports {condition}"),
        });
        rest = inner[close + 1..].trim_start();
    }
    if !rest.is_empty() {
        blocks.push(format!("@media {rest}"));
    }
    if blocks.is_empty() {
        return body.to_string();
    }
    let mut out = String::new();
    for block in &blocks {
        let _ = writeln!(out, "{block} {{");
    }
    out.push_str(body.trim_matches('\n'));
    out.push('\n');
    for _ in &blocks {
        out.push_str("}\n");
    }
    out
}

/// `css` without a leading byte order mark and `@charset "…";` rule.
fn without_charset(css: &str) -> &str {
    let css = css.strip_prefix('\u{feff}').unwrap_or(css);
    css.strip_prefix("@charset \"")
        .and_then(|rest| rest.find("\";").map(|end| &rest[end + 2..]))
        .unwrap_or(css)
}

/// Where the parenthesis that `text` follows closes, in `text`.
fn close_paren(text: &str) -> Option<usize> {
    function_close(text, 0)
}

/// `text` after `prefix`, which it starts with, in any case.
fn strip_prefix_ci<'a>(text: &'a str, prefix: &str) -> Option<&'a str> {
    let head = text.get(..prefix.len())?;
    head.eq_ignore_ascii_case(prefix)
        .then(|| &text[prefix.len()..])
}
