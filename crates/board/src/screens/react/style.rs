//! A `style` attribute as the object React's `style` prop takes:
//! `color: red; --brand: #f60; -webkit-line-clamp: 2` →
//! `{ color: "red", "--brand": "#f60", WebkitLineClamp: "2" }`.

use super::{Convert, js_string};
use crate::screens::{INDEX, css};

impl Convert<'_> {
    /// Declarations `text` as a JavaScript object literal, `None` when it has none. Each
    /// property's name is camel-cased (a custom property's kept, quoted); each value is a string,
    /// its `url(…)`s to local files pointed at the assets. A property given twice keeps its last
    /// value, as CSS does. `!important` can't be said in a `style` prop: it is dropped, and the
    /// property noted. An object with a custom property is cast to React's `CSSProperties`, whose
    /// type doesn't name them.
    pub(super) fn style(&mut self, text: &str) -> Option<String> {
        let mut entries: Vec<(String, String)> = Vec::new();
        for (property, value) in declarations(text) {
            let (value, important) = without_important(&value);
            if important && !self.important.contains(&property) {
                self.important.push(property.clone());
            }
            let value = css::rewrite_urls(value, |href| self.asset(INDEX, href));
            let key = key(&property);
            entries.retain(|(k, _)| *k != key);
            entries.push((key, value));
        }
        if entries.is_empty() {
            return None;
        }
        let fields: Vec<String> = entries
            .iter()
            .map(|(key, value)| format!("{key}: {}", js_string(value)))
            .collect();
        let object = format!("{{ {} }}", fields.join(", "));
        match entries.iter().any(|(key, _)| key.starts_with("\"--")) {
            true => {
                self.css_properties = true;
                Some(format!("{object} as CSSProperties"))
            }
            false => Some(object),
        }
    }
}

/// The declarations of a `style` attribute, each property (lowercased, unless it is a custom
/// property) with its value, both trimmed: split on `;` outside strings and parentheses,
/// comments gone, those without a `:` or a value left out.
pub(super) fn declarations(text: &str) -> Vec<(String, String)> {
    let mut found = Vec::new();
    let mut current = String::new();
    let mut depth = 0usize;
    let mut quote: Option<char> = None;
    let mut chars = text.chars().peekable();
    let mut take = |current: &mut String| {
        if let Some((property, value)) = current.split_once(':') {
            let (property, value) = (property.trim(), value.trim());
            if !property.is_empty() && !value.is_empty() {
                let property = match property.starts_with("--") {
                    true => property.to_string(),
                    false => property.to_ascii_lowercase(),
                };
                found.push((property, value.to_string()));
            }
        }
        current.clear();
    };
    while let Some(c) = chars.next() {
        if let Some(open) = quote {
            current.push(c);
            if c == '\\' {
                current.extend(chars.next());
            } else if c == open {
                quote = None;
            }
            continue;
        }
        match c {
            '"' | '\'' => {
                quote = Some(c);
                current.push(c);
            }
            '\\' => {
                current.push(c);
                current.extend(chars.next());
            }
            '/' if chars.peek() == Some(&'*') => {
                chars.next();
                let mut star = false;
                for d in chars.by_ref() {
                    if star && d == '/' {
                        break;
                    }
                    star = d == '*';
                }
            }
            '(' => {
                depth += 1;
                current.push(c);
            }
            ')' => {
                depth = depth.saturating_sub(1);
                current.push(c);
            }
            ';' if depth == 0 => take(&mut current),
            c => current.push(c),
        }
    }
    take(&mut current);
    found
}

/// `value` without a trailing `!important` (spaces allowed around the `!`), and whether it had
/// one.
fn without_important(value: &str) -> (&str, bool) {
    let value = value.trim_end();
    let cut = value.len().saturating_sub("important".len());
    let tail = value
        .get(cut..)
        .filter(|tail| tail.eq_ignore_ascii_case("important"));
    let head = tail.and_then(|_| value[..cut].trim_end().strip_suffix('!'));
    match head {
        Some(head) => (head.trim_end(), true),
        None => (value, false),
    }
}

/// A property's key in the object: a custom property quoted as written (`"--brand"`), a vendor
/// prefix as React spells it (`-webkit-` → `Webkit…`, `-moz-` → `Moz…`, `-ms-` → `ms…`), the
/// rest camel-cased (`background-image` → `backgroundImage`).
pub(super) fn key(property: &str) -> String {
    if property.starts_with("--") {
        return js_string(property);
    }
    let key = match property.strip_prefix('-') {
        Some(rest) => {
            let key = super::attrs::camel(rest);
            match key.starts_with("ms") {
                true => key,
                false => {
                    let mut chars = key.chars();
                    chars.next().map_or_else(String::new, |first| {
                        first.to_ascii_uppercase().to_string() + chars.as_str()
                    })
                }
            }
        }
        None => super::attrs::camel(property),
    };
    let identifier = key.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_' || c == '$')
        && key
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '$');
    match identifier {
        true => key,
        false => js_string(&key),
    }
}
