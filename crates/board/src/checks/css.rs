//! Style sheets as the screen checks read them: rules of simple selectors (type, class, id,
//! attribute, `*`, `:root`, chained with descendant and child combinators) and their
//! declarations. Rules inside `@media`, `@supports`, `@layer` and `@container` count (the checks
//! read the page as it is first shown); `@font-face`, `@keyframes` and the like hold none.
//! A selector with a pseudo-class or a sibling combinator styles another state or place than the
//! checks look at, so it matches no element; the focus check reads those itself ([`Sheet::text`]).

/// One element as selectors see it.
#[derive(Clone, Debug, Default)]
pub struct El {
    pub tag: String,
    pub id: Option<String>,
    pub classes: Vec<String>,
    pub attrs: Vec<(String, String)>,
}

/// A style rule: its selector text, each selector of its list, and its declarations.
#[derive(Clone, Debug)]
pub struct Rule {
    pub selector: String,
    selectors: Vec<Option<Complex>>,
    pub decls: Vec<(String, String, bool)>,
    /// The rule's place in the sheets, for the cascade.
    pub order: usize,
}

/// A selector of compounds, each with the combinator before it (`true` for `>`).
#[derive(Clone, Debug)]
struct Complex {
    parts: Vec<(Compound, bool)>,
    specificity: (u32, u32, u32),
}

#[derive(Clone, Debug, Default)]
struct Compound {
    tag: Option<String>,
    id: Option<String>,
    classes: Vec<String>,
    attrs: Vec<(String, Option<String>)>,
    root: bool,
}

/// Every style rule of the page's sheets, in order, and their whole text.
#[derive(Clone, Debug, Default)]
pub struct Sheet {
    pub rules: Vec<Rule>,
    pub text: String,
}

impl Sheet {
    /// The rules of `css`, after the ones already read.
    pub fn add(&mut self, css: &str) {
        let css = strip_comments(css);
        self.text.push_str(&css);
        self.text.push('\n');
        let mut order = self.rules.len();
        parse_block(&css, &mut self.rules, &mut order);
    }
}

impl Rule {
    /// The specificity this rule matches `path`'s last element with (`path` is the element's
    /// ancestors, then itself), or `None` when it doesn't.
    pub fn matches(&self, path: &[El]) -> Option<(u32, u32, u32)> {
        let found = self.selectors.iter().flatten();
        found
            .filter(|c| c.matches(path))
            .map(|c| c.specificity)
            .max()
    }
}

impl Complex {
    fn matches(&self, path: &[El]) -> bool {
        let Some((last, rest)) = self.parts.split_last() else {
            return false;
        };
        let Some((el, ancestors)) = path.split_last() else {
            return false;
        };
        if !last.0.matches(el, ancestors.is_empty()) {
            return false;
        }
        // Each compound before, from the right: a child of the one after it, or any ancestor.
        let mut child = last.1;
        let mut at = ancestors.len();
        for (compound, its_child) in rest.iter().rev() {
            let found = if child {
                (at > 0 && compound.matches(&ancestors[at - 1], at == 1)).then(|| at - 1)
            } else {
                (0..at)
                    .rev()
                    .find(|&i| compound.matches(&ancestors[i], i == 0))
            };
            match found {
                Some(i) => at = i,
                None => return false,
            }
            child = *its_child;
        }
        true
    }
}

impl Compound {
    fn matches(&self, el: &El, is_root: bool) -> bool {
        if self.root && !is_root {
            return false;
        }
        if self.tag.as_ref().is_some_and(|t| *t != el.tag) {
            return false;
        }
        if self.id.is_some() && self.id != el.id {
            return false;
        }
        if !self.classes.iter().all(|c| el.classes.contains(c)) {
            return false;
        }
        self.attrs.iter().all(|(name, value)| {
            let has = el.attrs.iter().find(|(n, _)| n == name);
            match (has, value) {
                (None, _) => false,
                (Some(_), None) => true,
                (Some((_, v)), Some(want)) => v == want,
            }
        })
    }
}

/// `css` without its comments.
fn strip_comments(css: &str) -> String {
    let mut out = String::with_capacity(css.len());
    let mut rest = css;
    while let Some(at) = rest.find("/*") {
        out.push_str(&rest[..at]);
        rest = rest[at + 2..]
            .find("*/")
            .map_or("", |end| &rest[at + 2 + end + 2..]);
    }
    out.push_str(rest);
    out
}

/// The index of the `}` that closes the block whose content starts at `from`, past strings.
fn block_end(css: &str, from: usize) -> usize {
    let mut depth = 0usize;
    let mut quote: Option<char> = None;
    for (i, c) in css[from..].char_indices() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (Some(_), _) => {}
            (None, '"' | '\'') => quote = Some(c),
            (None, '{') => depth += 1,
            (None, '}') if depth == 0 => return from + i,
            (None, '}') => depth -= 1,
            _ => {}
        }
    }
    css.len()
}

/// The rules of block `css` into `rules`.
fn parse_block(css: &str, rules: &mut Vec<Rule>, order: &mut usize) {
    let mut at = 0;
    while at < css.len() {
        let rest = &css[at..];
        let Some(open) = rest.find(['{', ';']) else {
            break;
        };
        let prelude = rest[..open].trim();
        if rest.as_bytes()[open] == b';' {
            // A statement at-rule (`@import`, `@charset`): no rules.
            at += open + 1;
            continue;
        }
        let start = at + open + 1;
        let end = block_end(css, start);
        let body = &css[start..end];
        if let Some(at_rule) = prelude.strip_prefix('@') {
            let name = at_rule.split_whitespace().next().unwrap_or_default();
            if matches!(
                name.to_ascii_lowercase().as_str(),
                "media" | "supports" | "layer" | "container" | "document"
            ) {
                parse_block(body, rules, order);
            }
        } else if !prelude.is_empty() {
            rules.push(Rule {
                selector: prelude.to_string(),
                selectors: prelude.split(',').map(|s| complex(s.trim())).collect(),
                decls: declarations(body),
                order: *order,
            });
            *order += 1;
        }
        at = end + 1;
    }
}

/// The declarations of `body` (or of a `style` attribute): name, value, `!important`.
pub fn declarations(body: &str) -> Vec<(String, String, bool)> {
    let mut out = Vec::new();
    let (mut depth, mut quote, mut start) = (0i32, None::<char>, 0);
    let mut parts = Vec::new();
    for (i, c) in body.char_indices() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (Some(_), _) => {}
            (None, '"' | '\'') => quote = Some(c),
            (None, '(') => depth += 1,
            (None, ')') => depth -= 1,
            (None, ';') if depth <= 0 => {
                parts.push(&body[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    parts.push(&body[start..]);
    for part in parts {
        let Some((name, value)) = part.split_once(':') else {
            continue;
        };
        let name = name.trim();
        let name = match name.starts_with("--") {
            true => name.to_string(),
            false => name.to_ascii_lowercase(),
        };
        let value = value.trim();
        let (value, important) = match value.to_ascii_lowercase().rfind("!important") {
            Some(at) => (value[..at].trim(), true),
            None => (value, false),
        };
        if !name.is_empty() && !value.is_empty() {
            out.push((name, value.to_string(), important));
        }
    }
    out
}

/// Selector `text` as compounds, or `None` for one the checks don't match (pseudo-classes,
/// pseudo-elements, sibling combinators).
fn complex(text: &str) -> Option<Complex> {
    if text.is_empty() || text.contains([':', '+', '~']) && !only_root(text) {
        return None;
    }
    let spaced = text.replace('>', " > ");
    let mut parts: Vec<(Compound, bool)> = Vec::new();
    let mut child = false;
    let mut spec = (0, 0, 0);
    for token in spaced.split_whitespace() {
        if token == ">" {
            child = true;
            continue;
        }
        let compound = compound(token, &mut spec)?;
        parts.push((compound, child));
        child = false;
    }
    (!parts.is_empty()).then_some(Complex {
        parts,
        specificity: spec,
    })
}

/// Whether the only colons in `text` are `:root`'s.
fn only_root(text: &str) -> bool {
    !text.replace(":root", "").contains([':', '+', '~'])
}

/// One compound selector (`button.primary[type=submit]`), its specificity added to `spec`.
fn compound(token: &str, spec: &mut (u32, u32, u32)) -> Option<Compound> {
    let mut out = Compound::default();
    let mut rest = token;
    if let Some(after) = rest.strip_prefix(":root") {
        out.root = true;
        spec.1 += 1;
        rest = after;
    }
    let head_end = rest.find(['.', '#', '[']).unwrap_or(rest.len());
    let head = &rest[..head_end];
    if !head.is_empty() && head != "*" {
        out.tag = Some(head.to_ascii_lowercase());
        spec.2 += 1;
    }
    rest = &rest[head_end..];
    while !rest.is_empty() {
        let kind = rest.as_bytes()[0];
        if kind == b'[' {
            let close = rest.find(']')?;
            let inner = &rest[1..close];
            let attr = match inner.split_once('=') {
                Some((name, value)) => {
                    let value = value.trim().trim_matches(['"', '\'']).to_string();
                    let name = name.trim();
                    // `~=`, `|=`, `^=`... compare otherwise than `=`.
                    if name.ends_with(['~', '|', '^', '$', '*']) {
                        return None;
                    }
                    (name.to_ascii_lowercase(), Some(value))
                }
                None => (inner.trim().to_ascii_lowercase(), None),
            };
            out.attrs.push(attr);
            spec.1 += 1;
            rest = &rest[close + 1..];
            continue;
        }
        let end = rest[1..]
            .find(['.', '#', '['])
            .map_or(rest.len(), |e| e + 1);
        let name = rest[1..end].to_string();
        if name.is_empty() {
            return None;
        }
        match kind {
            b'.' => {
                out.classes.push(name);
                spec.1 += 1;
            }
            b'#' => {
                out.id = Some(name);
                spec.0 += 1;
            }
            _ => return None,
        }
        rest = &rest[end..];
    }
    Some(out)
}
