//! Mermaid diagrams to SVG with Pele (https://pele.run), the MIT-licensed
//! renderer from Obsidian. Pele is JavaScript, so it runs in an embedded
//! QuickJS engine on its own thread; the bundle is compiled into the binary,
//! so diagrams render offline.
//!
//! Pele themes its SVG with CSS variables (and a `color-mix()` or two), which
//! the SVG rasterizer behind GPUI images (usvg) doesn't understand, so
//! [`flatten`] bakes the theme's colors into the markup. The result is a
//! standalone SVG that any viewer draws the same way, which is also what
//! "Save Diagram as SVG…" saves.

use std::collections::HashMap;
use std::ops::Range;
use std::sync::mpsc;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

/// Pele 1.12.0, `dist/pele.global.js` (defines the global `pele`).
const PELE_JS: &str = include_str!("../assets/pele/pele.global.js");

/// A runaway diagram is interrupted after this long.
const RENDER_TIMEOUT: Duration = Duration::from_secs(5);

/// The JS heap Pele may use.
const MEMORY_LIMIT: usize = 256 << 20;

/// Colors for Pele's `--pele-*` variables, as CSS colors (`#rrggbb`).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Palette {
    pub background: String,
    pub foreground: String,
    pub muted: String,
    pub line: String,
    pub surface: String,
    pub surface_alt: String,
    pub border: String,
    pub accent: String,
    pub critical: String,
}

impl Default for Palette {
    /// Pele's own light defaults.
    fn default() -> Self {
        Palette {
            background: "#ffffff".into(),
            foreground: "#1f1f1f".into(),
            muted: "#6e6e6e".into(),
            line: "#1f1f1f".into(),
            surface: "#f3f3f3".into(),
            surface_alt: "#e6e6e6".into(),
            border: "#8a8a8a".into(),
            accent: "#5b7bd5".into(),
            critical: "#e45756".into(),
        }
    }
}

impl Palette {
    fn vars(&self) -> [(&'static str, &str); 9] {
        [
            ("--pele-bg", &self.background),
            ("--pele-fg", &self.foreground),
            ("--pele-muted", &self.muted),
            ("--pele-line", &self.line),
            ("--pele-surface", &self.surface),
            ("--pele-surface-alt", &self.surface_alt),
            ("--pele-border", &self.border),
            ("--pele-accent", &self.accent),
            ("--pele-critical", &self.critical),
        ]
    }
}

/// A rendered diagram: a standalone SVG with the palette baked in.
#[derive(Debug, Clone)]
pub struct Diagram {
    pub svg: String,
    pub width: f64,
    pub height: f64,
    /// Pele's name for the diagram type, such as `flowchart`.
    pub kind: String,
}

/// Renders Mermaid `source`. Blocks until done (well under a millisecond for
/// typical diagrams; the first call also loads Pele, about 60 ms), so call it
/// off the UI thread. The error is Pele's message, which quotes the source.
pub fn render(source: &str, palette: &Palette) -> Result<Diagram, String> {
    let raw = run(source.to_owned())?;
    Ok(Diagram {
        svg: flatten(&raw.svg, palette),
        width: raw.width,
        height: raw.height,
        kind: raw.kind,
    })
}

struct Raw {
    svg: String,
    width: f64,
    height: f64,
    kind: String,
}

struct Job {
    source: String,
    reply: mpsc::Sender<Result<Raw, String>>,
}

/// Sends `source` to the Pele thread, starting it on first use.
fn run(source: String) -> Result<Raw, String> {
    static WORKER: OnceLock<mpsc::Sender<Job>> = OnceLock::new();
    let worker = WORKER.get_or_init(|| {
        let (tx, rx) = mpsc::channel::<Job>();
        std::thread::Builder::new()
            .name("sawit-mermaid".into())
            .spawn(move || worker(rx))
            .expect("failed to spawn mermaid thread");
        tx
    });
    let stopped = || "The diagram renderer stopped.".to_string();
    let (reply, result) = mpsc::channel();
    worker.send(Job { source, reply }).map_err(|_| stopped())?;
    result.recv().map_err(|_| stopped())?
}

fn worker(jobs: mpsc::Receiver<Job>) {
    use rquickjs::{Context, Runtime};

    let deadline: Arc<Mutex<Option<Instant>>> = Arc::default();
    let engine = (|| {
        let rt = Runtime::new().map_err(|e| e.to_string())?;
        rt.set_memory_limit(MEMORY_LIMIT);
        let d = deadline.clone();
        rt.set_interrupt_handler(Some(Box::new(move || {
            d.lock()
                .ok()
                .and_then(|d| *d)
                .is_some_and(|d| Instant::now() > d)
        })));
        let cx = Context::full(&rt).map_err(|e| e.to_string())?;
        cx.with(|cx| {
            cx.eval::<(), _>(PELE_JS)
                .map_err(|e| exception(&cx, e, "Pele failed to load"))
        })?;
        Ok::<_, String>((rt, cx))
    })();
    let set_deadline = |at| {
        if let Ok(mut d) = deadline.lock() {
            *d = at;
        }
    };
    for job in jobs {
        let result = match &engine {
            Ok((_, cx)) => {
                set_deadline(Some(Instant::now() + RENDER_TIMEOUT));
                let result = cx.with(|cx| call_render(&cx, &job.source));
                set_deadline(None);
                result
            }
            Err(e) => Err(e.clone()),
        };
        let _ = job.reply.send(result);
    }
}

fn call_render(cx: &rquickjs::Ctx<'_>, source: &str) -> Result<Raw, String> {
    use rquickjs::{Function, Object};

    let err = |e| exception(cx, e, "The diagram can't be drawn");
    let pele: Object = cx.globals().get("pele").map_err(err)?;
    let render: Function = pele.get("render").map_err(err)?;
    let options = Object::new(cx.clone()).map_err(err)?;
    // A fixed pixel size: the preview scales it down to fit.
    options.set("responsive", false).map_err(err)?;
    options.set("idPrefix", "mermaid").map_err(err)?;
    let result: Object = render.call((source, options)).map_err(err)?;
    Ok(Raw {
        svg: result.get("svg").map_err(err)?,
        width: result.get("width").map_err(err)?,
        height: result.get("height").map_err(err)?,
        kind: result.get("type").unwrap_or_default(),
    })
}

/// The pending JS exception's message, or `fallback` with the engine error.
fn exception(cx: &rquickjs::Ctx<'_>, e: rquickjs::Error, fallback: &str) -> String {
    if !e.is_exception() {
        return format!("{fallback}: {e}");
    }
    let thrown = cx.catch();
    let message = thrown
        .as_object()
        .and_then(|o| o.get::<_, String>("message").ok())
        .or_else(|| thrown.as_string().and_then(|s| s.to_string().ok()))
        .filter(|m| !m.is_empty());
    match message {
        // QuickJS throws "interrupted" once the deadline passes.
        Some(m) if m == "interrupted" => format!("{fallback}: it took too long."),
        Some(m) => m,
        None => format!("{fallback}."),
    }
}

// --- Flattening -----------------------------------------------------------

/// Bakes `palette` into Pele's SVG: resolves every `var()` (custom properties
/// are declared in `style` attributes, the theme's `--pele-*` win), evaluates
/// `color-mix()`, drops the declarations, and paints the background so the
/// file reads the same on any canvas.
pub fn flatten(svg: &str, palette: &Palette) -> String {
    let mut vars: HashMap<String, String> = palette
        .vars()
        .into_iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
    rewrite_attrs(svg, |name, value| {
        if name == "style" {
            for (k, v) in declarations(value).filter_map(custom_property) {
                vars.entry(k.to_string()).or_insert_with(|| v.to_string());
            }
        }
        None
    });
    let flat = rewrite_attrs(svg, |name, value| {
        let kept;
        let value = if name == "style" {
            kept = declarations(value)
                .filter(|d| custom_property(d).is_none())
                .collect::<Vec<_>>()
                .join(";");
            kept.as_str()
        } else if value.contains("var(") || value.contains("color-mix(") {
            value
        } else {
            return None;
        };
        Some(mix_colors(&resolve(value, &vars, 0)))
    });
    with_background(&flat, &palette.background)
}

/// Calls `f(attribute, value)` for every quoted attribute value in the
/// markup (not text content); a `Some` replaces the value.
fn rewrite_attrs(svg: &str, mut f: impl FnMut(&str, &str) -> Option<String>) -> String {
    let bytes = svg.as_bytes();
    let mut out = String::with_capacity(svg.len());
    let mut copied = 0;
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b'<' {
            i += 1;
            continue;
        }
        let mut name = "";
        i += 1;
        while i < bytes.len() && bytes[i] != b'>' {
            match bytes[i] {
                b'=' => {
                    let before = svg[..i].trim_end();
                    let start = before
                        .rfind(|c: char| c.is_whitespace() || c == '<')
                        .map_or(0, |p| p + 1);
                    name = &before[start..];
                    i += 1;
                }
                quote @ (b'"' | b'\'') => {
                    let start = i + 1;
                    let end = svg[start..]
                        .find(quote as char)
                        .map_or(svg.len(), |n| start + n);
                    if let Some(new) = f(name, &svg[start..end]) {
                        out.push_str(&svg[copied..start]);
                        out.push_str(&new);
                        copied = end;
                    }
                    i = end + 1;
                }
                _ => i += 1,
            }
        }
    }
    out.push_str(&svg[copied.min(svg.len())..]);
    out
}

/// The `;`-separated declarations of a `style` value (not inside parens).
fn declarations(style: &str) -> impl Iterator<Item = &str> {
    split_top(style, ';')
        .into_iter()
        .map(str::trim)
        .filter(|d| !d.is_empty())
}

fn custom_property(declaration: &str) -> Option<(&str, &str)> {
    let (k, v) = declaration.split_once(':')?;
    let k = k.trim();
    k.starts_with("--").then(|| (k, v.trim()))
}

/// Splits on `sep` outside parentheses.
fn split_top(s: &str, sep: char) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut depth = 0usize;
    let mut start = 0;
    for (i, c) in s.char_indices() {
        match c {
            '(' => depth += 1,
            ')' => depth = depth.saturating_sub(1),
            c if c == sep && depth == 0 => {
                parts.push(&s[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    parts.push(&s[start..]);
    parts
}

/// Length of `s` up to the `)` closing an already-open paren.
fn closing_paren(s: &str) -> Option<usize> {
    let mut depth = 0usize;
    for (i, c) in s.char_indices() {
        match c {
            '(' => depth += 1,
            ')' if depth == 0 => return Some(i),
            ')' => depth -= 1,
            _ => {}
        }
    }
    None
}

/// Replaces each `var(--name[, fallback])` with its value.
fn resolve(value: &str, vars: &HashMap<String, String>, depth: u8) -> String {
    if depth > 8 || !value.contains("var(") {
        return value.to_string();
    }
    let mut out = String::with_capacity(value.len());
    let mut rest = value;
    while let Some(at) = rest.find("var(") {
        out.push_str(&rest[..at]);
        let inner_start = at + "var(".len();
        let Some(len) = closing_paren(&rest[inner_start..]) else {
            out.push_str(&rest[at..]);
            return out;
        };
        let inner = &rest[inner_start..inner_start + len];
        let (name, fallback) = match inner.split_once(',') {
            Some((name, fallback)) => (name.trim(), Some(fallback.trim())),
            None => (inner.trim(), None),
        };
        let replacement = vars.get(name).map(String::as_str).or(fallback);
        out.push_str(&resolve(replacement.unwrap_or_default(), vars, depth + 1));
        rest = &rest[inner_start + len + 1..];
    }
    out.push_str(rest);
    out
}

/// Evaluates `color-mix(in srgb, A p%, B [q%])` into `#rrggbb`.
fn mix_colors(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut rest = value;
    while let Some(at) = rest.find("color-mix(") {
        out.push_str(&rest[..at]);
        let inner_start = at + "color-mix(".len();
        let Some(len) = closing_paren(&rest[inner_start..]) else {
            out.push_str(&rest[at..]);
            return out;
        };
        let inner = mix_colors(&rest[inner_start..inner_start + len]);
        out.push_str(&mix(&inner).unwrap_or_else(|| format!("color-mix({inner})")));
        rest = &rest[inner_start + len + 1..];
    }
    out.push_str(rest);
    out
}

fn mix(args: &str) -> Option<String> {
    let parts = split_top(args, ',');
    let [space, a, b] = parts.as_slice() else {
        return None;
    };
    if space.trim() != "in srgb" {
        return None;
    }
    let (a, pa) = color_stop(a)?;
    let (b, pb) = color_stop(b)?;
    let wa = match (pa, pb) {
        (Some(pa), _) => pa,
        (None, Some(pb)) => 1.0 - pb,
        (None, None) => 0.5,
    };
    let channel = |i: usize| (a[i] * wa + b[i] * (1.0 - wa)).round().clamp(0.0, 255.0) as u8;
    Some(format!(
        "#{:02x}{:02x}{:02x}",
        channel(0),
        channel(1),
        channel(2)
    ))
}

/// `#rrggbb 40%` → color and weight.
fn color_stop(s: &str) -> Option<([f64; 3], Option<f64>)> {
    let s = s.trim();
    let (color, weight) = match s.rsplit_once(char::is_whitespace) {
        Some((c, w)) if w.ends_with('%') => (
            c.trim(),
            Some(w.trim_end_matches('%').parse::<f64>().ok()? / 100.0),
        ),
        _ => (s, None),
    };
    Some((parse_hex(color)?, weight))
}

fn parse_hex(color: &str) -> Option<[f64; 3]> {
    let hex = color.strip_prefix('#')?;
    let digits: Vec<u8> = hex
        .chars()
        .map(|c| c.to_digit(16).map(|d| d as u8))
        .collect::<Option<_>>()?;
    let rgb = match digits.len() {
        3 | 4 => [digits[0] * 17, digits[1] * 17, digits[2] * 17],
        6 | 8 => [
            digits[0] * 16 + digits[1],
            digits[2] * 16 + digits[3],
            digits[4] * 16 + digits[5],
        ],
        _ => return None,
    };
    Some(rgb.map(f64::from))
}

/// Inserts a full-size background rect as the root's first child.
fn with_background(svg: &str, color: &str) -> String {
    let Some(open) = svg.find("<svg") else {
        return svg.to_string();
    };
    let mut quote = None;
    for (i, c) in svg[open..].char_indices() {
        match (c, quote) {
            ('"' | '\'', None) => quote = Some(c),
            (c, Some(q)) if c == q => quote = None,
            ('>', None) => {
                let at = open + i + 1;
                return format!(
                    "{}<rect width=\"100%\" height=\"100%\" fill=\"{color}\"/>{}",
                    &svg[..at],
                    &svg[at..]
                );
            }
            _ => {}
        }
    }
    svg.to_string()
}

// --- Markdown --------------------------------------------------------------

/// A fenced code block tagged `mermaid` in a Markdown document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fence {
    /// Byte range of the whole block, fences included.
    pub range: Range<usize>,
    pub source: String,
}

/// Top-level ```` ```mermaid ```` / `~~~mermaid` blocks, in order. Fences
/// inside other code blocks are skipped; an unclosed block runs to the end.
pub fn fences(markdown: &str) -> Vec<Fence> {
    struct Open {
        start: usize,
        marker: char,
        len: usize,
        mermaid: bool,
        body: usize,
    }
    let mut found = Vec::new();
    let mut open: Option<Open> = None;
    let mut offset = 0;
    for line in markdown.split_inclusive('\n') {
        let line_start = offset;
        offset += line.len();
        let text = line.trim_end_matches(['\n', '\r']);
        let indent = text.len() - text.trim_start_matches(' ').len();
        let trimmed = &text[indent..];
        let marker = trimmed.chars().next().filter(|c| matches!(c, '`' | '~'));
        let run = marker.map_or(0, |m| trimmed.len() - trimmed.trim_start_matches(m).len());
        let fence = marker.filter(|_| indent <= 3 && run >= 3);
        match (&open, fence) {
            (None, Some(marker)) => {
                let info = trimmed[run..].trim();
                // Not a fence: a backtick fence's info string has no backticks.
                if marker == '`' && info.contains('`') {
                    continue;
                }
                let lang = info.split_whitespace().next().unwrap_or_default();
                open = Some(Open {
                    start: line_start,
                    marker,
                    len: run,
                    mermaid: lang.eq_ignore_ascii_case("mermaid"),
                    body: offset,
                });
            }
            (Some(o), Some(marker))
                if marker == o.marker && run >= o.len && trimmed[run..].trim().is_empty() =>
            {
                if o.mermaid {
                    found.push(Fence {
                        range: o.start..offset,
                        source: markdown[o.body..line_start].to_string(),
                    });
                }
                open = None;
            }
            _ => {}
        }
    }
    if let Some(o) = open.filter(|o| o.mermaid) {
        found.push(Fence {
            range: o.start..markdown.len(),
            source: markdown[o.body..].to_string(),
        });
    }
    found
}

/// Image URL prefix for diagram `<index>` of a rewritten document.
pub const IMAGE_SCHEME: &str = "sawit-mermaid:";
/// Link URL prefix that saves diagram `<index>` as `.svg`.
pub const SAVE_SCHEME: &str = "sawit-mermaid-save:";

/// Replaces each fence with the diagram image (`sawit-mermaid:<index>`) and
/// a "Save Diagram as SVG…" link (`sawit-mermaid-save:<index>`), or with Pele's
/// error when it couldn't be drawn. `results` lines up with `fences`.
pub fn rewrite_markdown(
    markdown: &str,
    fences: &[Fence],
    results: &[Result<Diagram, String>],
) -> String {
    let mut out = String::with_capacity(markdown.len());
    let mut copied = 0;
    for (ix, (fence, result)) in fences.iter().zip(results).enumerate() {
        out.push_str(&markdown[copied..fence.range.start]);
        match result {
            Ok(_) => out.push_str(&format!(
                "![Mermaid diagram]({IMAGE_SCHEME}{ix})\n\n[Save Diagram as SVG…]({SAVE_SCHEME}{ix})\n"
            )),
            Err(e) => {
                let ticks = "`".repeat(longest_run(e, '`').max(2) + 1);
                out.push_str(&format!(
                    "**Mermaid diagram can't be drawn:**\n\n{ticks}text\n{}\n{ticks}\n",
                    e.trim_end()
                ));
            }
        }
        out.push('\n');
        copied = fence.range.end;
    }
    out.push_str(&markdown[copied..]);
    out
}

fn longest_run(s: &str, c: char) -> usize {
    s.split(|x| x != c).map(str::len).max().unwrap_or(0)
}

/// The diagram index in a `sawit-mermaid:` / `sawit-mermaid-save:` URL.
pub fn index_in(url: &str, scheme: &str) -> Option<usize> {
    url.strip_prefix(scheme)?.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flatten_resolves_vars_and_mixes() {
        let svg = r##"<svg class="pele" style="max-width:100%;--_bg:var(--pele-bg,#fff);--_a:var(--pele-surface-alt,#e6e6e6);--_r:var(--pele-radius,4px);color:var(--_fg)" fill="var(--_fg)"><rect style="rx:var(--_r)" fill="color-mix(in srgb,var(--_a) 50%,var(--_bg))"/><text>var(--kept) a;--b:c</text><path fill="var(--pele-series-1,#4c78a8)"/></svg>"##;
        let palette = Palette {
            background: "#000000".into(),
            surface_alt: "#ffffff".into(),
            ..Palette::default()
        };
        let out = flatten(svg, &palette);
        assert!(!out.contains("var(--_"), "{out}");
        assert!(!out.contains("color-mix"), "{out}");
        // `--_fg` isn't declared here and has no fallback.
        assert!(out.contains(r#"style="max-width:100%;color:""#), "{out}");
        assert!(out.contains(r#"style="rx:4px""#), "{out}");
        assert!(out.contains(r##"fill="#808080""##), "{out}");
        assert!(out.contains(r##"fill="#4c78a8""##), "{out}");
        // Text content is untouched.
        assert!(out.contains("<text>var(--kept) a;--b:c</text>"), "{out}");
        assert!(out.contains(r##"<rect width="100%" height="100%" fill="#000000"/>"##));
    }

    #[test]
    fn mix_weights() {
        assert_eq!(
            mix_colors("color-mix(in srgb,#ffffff 25%,#000000)"),
            "#404040"
        );
        assert_eq!(mix_colors("color-mix(in srgb, #fff, #000 25%)"), "#bfbfbf");
        // Anything else is left for the renderer to ignore.
        assert_eq!(
            mix_colors("color-mix(in oklch,red,blue)"),
            "color-mix(in oklch,red,blue)"
        );
    }

    #[test]
    fn finds_mermaid_fences() {
        let md = "# T\n\n```mermaid\ngraph LR\n  A-->B\n```\n\n````md\n```mermaid\nnot me\n```\n````\n\n  ~~~ Mermaid title\npie\n~~~\n\n```mermaid\nunclosed";
        let found = fences(md);
        assert_eq!(found.len(), 3);
        assert_eq!(found[0].source, "graph LR\n  A-->B\n");
        assert_eq!(
            &md[found[0].range.clone()],
            "```mermaid\ngraph LR\n  A-->B\n```\n"
        );
        assert_eq!(found[1].source, "pie\n");
        assert_eq!(found[2].source, "unclosed");
    }

    #[test]
    fn rewrites_fences() {
        let md = "a\n```mermaid\nx\n```\nb\n```mermaid\ny\n```\n";
        let found = fences(md);
        let diagram = Diagram {
            svg: String::new(),
            width: 1.,
            height: 1.,
            kind: "flowchart".into(),
        };
        let out = rewrite_markdown(md, &found, &[Ok(diagram), Err("bad ``` thing".into())]);
        assert_eq!(
            out,
            "a\n![Mermaid diagram](sawit-mermaid:0)\n\n[Save Diagram as SVG…](sawit-mermaid-save:0)\n\nb\n**Mermaid diagram can't be drawn:**\n\n````text\nbad ``` thing\n````\n\n"
        );
        assert_eq!(index_in("sawit-mermaid-save:12", SAVE_SCHEME), Some(12));
        assert_eq!(index_in("sawit-mermaid:x", IMAGE_SCHEME), None);
    }

    #[test]
    fn renders_and_reports_errors() {
        let d = render("flowchart LR\n  A[Hello] --> B[World]", &Palette::default()).unwrap();
        assert_eq!(d.kind, "flowchart");
        assert!(d.width > 0. && d.height > 0.);
        assert!(d.svg.starts_with("<svg") && !d.svg.contains("var("));
        assert!(render("flowchart LR\n  A[unclosed --> B", &Palette::default()).is_err());
        assert!(render("notADiagram", &Palette::default()).is_err());
    }
}
