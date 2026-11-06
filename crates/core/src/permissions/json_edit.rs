//! Editing a JSON file in place: the spans of its objects, arrays and values, read without
//! changing a byte, and edits that splice text in or out where they belong. Everything else in
//! the file (its keys, their order, its indentation and line breaks, an array kept on one line)
//! stays as the user wrote it. Comments (`//`, `/* */`) are skipped over and kept.

use std::ops::Range;

/// A JSON value and where it is in the text.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct Node {
    pub span: Range<usize>,
    pub kind: Kind,
}

#[derive(Debug, Clone, PartialEq)]
pub(super) enum Kind {
    /// Its members: the key (decoded), where the key starts, and its value.
    Object(Vec<(String, usize, Node)>),
    Array(Vec<Node>),
    /// A string, decoded.
    Str(String),
    /// A number, `true`, `false` or `null`.
    Other,
}

/// Parses `text` (one JSON value, with whitespace and comments around it). `Err` says where
/// it went wrong.
pub(super) fn parse(text: &str) -> Result<Node, String> {
    let b = text.as_bytes();
    let mut p = Parser { b, i: 0 };
    p.skip();
    let node = p.value()?;
    p.skip();
    if p.i != b.len() {
        return Err(p.error("text after the end of the JSON"));
    }
    Ok(node)
}

struct Parser<'a> {
    b: &'a [u8],
    i: usize,
}

impl Parser<'_> {
    fn error(&self, what: &str) -> String {
        let line = self.b[..self.i.min(self.b.len())]
            .iter()
            .filter(|c| **c == b'\n')
            .count()
            + 1;
        format!("{what}, on line {line}")
    }

    /// Skips whitespace and comments.
    fn skip(&mut self) {
        loop {
            while self.i < self.b.len() && self.b[self.i].is_ascii_whitespace() {
                self.i += 1;
            }
            if self.b[self.i..].starts_with(b"//") {
                while self.i < self.b.len() && self.b[self.i] != b'\n' {
                    self.i += 1;
                }
            } else if self.b[self.i..].starts_with(b"/*") {
                self.i += 2;
                while self.i < self.b.len() && !self.b[self.i..].starts_with(b"*/") {
                    self.i += 1;
                }
                self.i = (self.i + 2).min(self.b.len());
            } else {
                return;
            }
        }
    }

    fn value(&mut self) -> Result<Node, String> {
        let start = self.i;
        match self.b.get(self.i) {
            Some(b'{') => {
                self.i += 1;
                let mut members = Vec::new();
                self.skip();
                if self.b.get(self.i) == Some(&b'}') {
                    self.i += 1;
                    return Ok(Node {
                        span: start..self.i,
                        kind: Kind::Object(members),
                    });
                }
                loop {
                    self.skip();
                    let key_node = self.string()?;
                    let key_at = key_node.span.start;
                    let Kind::Str(key) = key_node.kind else {
                        unreachable!()
                    };
                    self.skip();
                    if self.b.get(self.i) != Some(&b':') {
                        return Err(self.error("a `:` is missing"));
                    }
                    self.i += 1;
                    self.skip();
                    let value = self.value()?;
                    members.push((key, key_at, value));
                    self.skip();
                    match self.b.get(self.i) {
                        Some(b',') => {
                            self.i += 1;
                            self.skip();
                            // A trailing comma (JSONC).
                            if self.b.get(self.i) == Some(&b'}') {
                                self.i += 1;
                                break;
                            }
                        }
                        Some(b'}') => {
                            self.i += 1;
                            break;
                        }
                        _ => return Err(self.error("a `,` or `}` is missing")),
                    }
                }
                Ok(Node {
                    span: start..self.i,
                    kind: Kind::Object(members),
                })
            }
            Some(b'[') => {
                self.i += 1;
                let mut items = Vec::new();
                self.skip();
                if self.b.get(self.i) == Some(&b']') {
                    self.i += 1;
                    return Ok(Node {
                        span: start..self.i,
                        kind: Kind::Array(items),
                    });
                }
                loop {
                    self.skip();
                    items.push(self.value()?);
                    self.skip();
                    match self.b.get(self.i) {
                        Some(b',') => {
                            self.i += 1;
                            self.skip();
                            if self.b.get(self.i) == Some(&b']') {
                                self.i += 1;
                                break;
                            }
                        }
                        Some(b']') => {
                            self.i += 1;
                            break;
                        }
                        _ => return Err(self.error("a `,` or `]` is missing")),
                    }
                }
                Ok(Node {
                    span: start..self.i,
                    kind: Kind::Array(items),
                })
            }
            Some(b'"') => self.string(),
            Some(_) => {
                while self.i < self.b.len()
                    && !matches!(self.b[self.i], b',' | b'}' | b']' | b'/')
                    && !self.b[self.i].is_ascii_whitespace()
                {
                    self.i += 1;
                }
                let word = std::str::from_utf8(&self.b[start..self.i]).unwrap_or("");
                if word.is_empty() || serde_json::from_str::<serde_json::Value>(word).is_err() {
                    return Err(self.error("a value isn't JSON"));
                }
                Ok(Node {
                    span: start..self.i,
                    kind: Kind::Other,
                })
            }
            None => Err(self.error("the JSON ends early")),
        }
    }

    fn string(&mut self) -> Result<Node, String> {
        let start = self.i;
        if self.b.get(self.i) != Some(&b'"') {
            return Err(self.error("a string is missing"));
        }
        self.i += 1;
        while self.i < self.b.len() && self.b[self.i] != b'"' {
            if self.b[self.i] == b'\\' {
                self.i += 1;
            }
            self.i += 1;
        }
        if self.i >= self.b.len() {
            return Err(self.error("a string isn't closed"));
        }
        self.i += 1;
        let raw = std::str::from_utf8(&self.b[start..self.i]).map_err(|e| e.to_string())?;
        let text: String =
            serde_json::from_str(raw).map_err(|_| self.error("a string isn't JSON"))?;
        Ok(Node {
            span: start..self.i,
            kind: Kind::Str(text),
        })
    }
}

impl Node {
    pub fn member(&self, key: &str) -> Option<&Node> {
        match &self.kind {
            Kind::Object(members) => members.iter().find(|(k, _, _)| k == key).map(|(_, _, v)| v),
            _ => None,
        }
    }
}

/// The leading whitespace of the line `pos` is on.
fn indent_at(text: &str, pos: usize) -> &str {
    let start = text[..pos].rfind('\n').map_or(0, |n| n + 1);
    let line = &text[start..];
    &line[..line.len() - line.trim_start_matches([' ', '\t']).len()]
}

/// One level of indentation as the file uses it: the first indented line's, else two spaces.
fn unit(text: &str) -> String {
    text.lines()
        .map(|l| &l[..l.len() - l.trim_start_matches([' ', '\t']).len()])
        .find(|w| !w.is_empty() && w.len() <= 8)
        .unwrap_or("  ")
        .to_string()
}

/// `value` (compact JSON) written as a block at `indent`: objects and arrays one member or item
/// a line, as a person would.
fn pretty(value: &serde_json::Value, indent: &str, unit: &str) -> String {
    let inner = format!("{indent}{unit}");
    match value {
        serde_json::Value::Object(map) if !map.is_empty() => {
            let lines: Vec<String> = map
                .iter()
                .map(|(k, v)| {
                    format!(
                        "{inner}{}: {}",
                        serde_json::to_string(k).unwrap_or_default(),
                        pretty(v, &inner, unit)
                    )
                })
                .collect();
            format!("{{\n{}\n{indent}}}", lines.join(",\n"))
        }
        serde_json::Value::Array(items) if !items.is_empty() => {
            let lines: Vec<String> = items
                .iter()
                .map(|v| format!("{inner}{}", pretty(v, &inner, unit)))
                .collect();
            format!("[\n{}\n{indent}]", lines.join(",\n"))
        }
        other => serde_json::to_string(other).unwrap_or_default(),
    }
}

/// The separator a container uses before its last element (a comma, then the line break and
/// indentation, or a space), or `None` when it has a single element; then the text between its
/// opening bracket and its first element.
fn separators(text: &str, open: usize, elements: &[Range<usize>]) -> (Option<String>, String) {
    let lead = text[open + 1..elements[0].start].to_string();
    let sep = (elements.len() > 1).then(|| {
        let last = &elements[elements.len() - 1];
        let prev = &elements[elements.len() - 2];
        text[prev.end..last.start].to_string()
    });
    (sep, lead)
}

/// The separator for a second element, from the text before the first: a comma, then that
/// line break and indentation, or a space for a container on one line.
fn first_separator(lead: &str) -> String {
    if lead.contains('\n') {
        format!(",{}", &lead[lead.rfind('\n').unwrap_or(0)..])
    } else {
        ", ".into()
    }
}

/// `text` with `item` (a JSON value) added at the end of the array `array`.
pub(super) fn push_item(text: &str, array: &Node, item: &serde_json::Value) -> String {
    let Kind::Array(items) = &array.kind else {
        return text.to_string();
    };
    let unit = unit(text);
    let base = indent_at(text, array.span.start).to_string();
    if items.is_empty() {
        let inner = format!("{base}{unit}");
        let new = format!("[\n{inner}{}\n{base}]", pretty(item, &inner, &unit));
        return splice(text, array.span.clone(), &new);
    }
    let spans: Vec<Range<usize>> = items.iter().map(|n| n.span.clone()).collect();
    let (sep, lead) = separators(text, array.span.start, &spans);
    // Between items, as the file has it; else the whitespace before the first, after a comma.
    let sep = sep.unwrap_or_else(|| first_separator(&lead));
    let indent = indent_at(text, spans[spans.len() - 1].start).to_string();
    let at = spans[spans.len() - 1].end;
    splice(
        text,
        at..at,
        &format!("{sep}{}", pretty(item, &indent, &unit)),
    )
}

/// `text` without item `ix` of the array `array`, with its comma; a last item leaves `[]`.
pub(super) fn remove_item(text: &str, array: &Node, ix: usize) -> String {
    let Kind::Array(items) = &array.kind else {
        return text.to_string();
    };
    match items.len() {
        0 => text.to_string(),
        1 => splice(text, array.span.clone(), "[]"),
        n if ix + 1 < n => splice(text, items[ix].span.start..items[ix + 1].span.start, ""),
        _ => splice(text, items[ix - 1].span.end..items[ix].span.end, ""),
    }
}

/// `text` with the member `key: value` added at the end of the object `object`.
pub(super) fn push_member(
    text: &str,
    object: &Node,
    key: &str,
    value: &serde_json::Value,
) -> String {
    let Kind::Object(members) = &object.kind else {
        return text.to_string();
    };
    let unit = unit(text);
    let base = indent_at(text, object.span.start).to_string();
    let key = serde_json::to_string(key).unwrap_or_default();
    if members.is_empty() {
        let inner = format!("{base}{unit}");
        let new = format!(
            "{{\n{inner}{key}: {}\n{base}}}",
            pretty(value, &inner, &unit)
        );
        return splice(text, object.span.clone(), &new);
    }
    let starts: Vec<Range<usize>> = members.iter().map(|(_, at, v)| *at..v.span.end).collect();
    let (sep, lead) = separators(text, object.span.start, &starts);
    let sep = sep.unwrap_or_else(|| first_separator(&lead));
    let indent = indent_at(text, starts[starts.len() - 1].start).to_string();
    let at = starts[starts.len() - 1].end;
    splice(
        text,
        at..at,
        &format!("{sep}{key}: {}", pretty(value, &indent, &unit)),
    )
}

fn splice(text: &str, range: Range<usize>, with: &str) -> String {
    let mut out = String::with_capacity(text.len() + with.len());
    out.push_str(&text[..range.start]);
    out.push_str(with);
    out.push_str(&text[range.end..]);
    out
}
