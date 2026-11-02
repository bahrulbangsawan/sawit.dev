//! Sawit's changes to a mesurer workspace (`PagedWorkspaceStore` JSON), made for an agent
//! (`browser_mesurer set` and `clear`) or the Annotate panel (a comment's row removed, and put
//! back by Undo). The app writes the result to Sawit's files and hands it to mesurer
//! (`SawitMesurer.apply`), which shows it as it shows what it loads: so every change here keeps
//! to what mesurer's own normalizing (`core/persistence.ts`) reads, or it would be dropped.
//! Comments' words are the user's: nothing here writes one.

use serde_json::{Map, Value, json};

/// What a page's artifacts hold when there are none (`EMPTY_PAGE_ARTIFACTS`); mesurer drops a
/// page whose `guides`, `selectedGuideIds`, `measurements` or `heldDistances` isn't a list.
const PAGE_LISTS: [&str; 12] = [
    "guides",
    "selectedGuideIds",
    "arrows",
    "selectedArrowIds",
    "selectedPenStrokeIds",
    "selectedTextIds",
    "penStrokes",
    "textAnnotations",
    "measurements",
    "heldDistances",
    "comments",
    "layoutGuides",
];

/// The tools `set {tool}` takes: mesurer's tool modes (X-ray and rulers are switches of their
/// own), and the names its toolbar gives some of them.
const TOOLS: [(&str, &str); 12] = [
    ("none", "none"),
    ("select", "select"),
    ("inspect", "select"),
    ("selection", "selection"),
    ("guides", "guides"),
    ("arrows", "arrows"),
    ("arrow", "arrows"),
    ("pen", "pen"),
    ("text", "text"),
    ("comments", "comments"),
    ("comment", "comments"),
    ("annotate-select", "selection"),
];

/// What `set` takes, and what `clear` clears.
pub const SET_KEYS: [&str; 6] = [
    "xray",
    "rulers",
    "tool",
    "minimized",
    "guides",
    "layout_guides",
];
pub const CLEAR_WHAT: [&str; 6] = [
    "drawings",
    "measurements",
    "guides",
    "layout_guides",
    "comments",
    "all",
];

/// A workspace for an origin mesurer has saved nothing for: open, with no tool.
pub fn new_envelope() -> Value {
    json!({
        "enabled": true,
        "minimized": false,
        "xrayVisible": false,
        "toolMode": "none",
        "rulersVisible": false,
        "guideOrientation": "vertical",
        "pages": {},
    })
}

/// An id for a guide or a layout guide Sawit adds, unique in the session.
pub fn new_id() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    format!(
        "sawit-{:x}-{}",
        nanos & 0xffff_ffff_ffff,
        NEXT.fetch_add(1, Ordering::Relaxed)
    )
}

/// The envelope's session chrome, made whole where mesurer would refuse it.
fn chrome(envelope: &mut Value) -> &mut Map<String, Value> {
    if !envelope.is_object() {
        *envelope = new_envelope();
    }
    let map = envelope.as_object_mut().expect("an object");
    let defaults = new_envelope();
    for key in ["enabled", "toolMode", "rulersVisible", "guideOrientation"] {
        let valid = match key {
            "toolMode" => map.get(key).is_some_and(Value::is_string),
            "guideOrientation" => map
                .get(key)
                .is_some_and(|v| v == "vertical" || v == "horizontal"),
            _ => map.get(key).is_some_and(Value::is_boolean),
        };
        if !valid {
            map.insert(key.into(), defaults[key].clone());
        }
    }
    if !map.get("pages").is_some_and(Value::is_object) {
        map.insert("pages".into(), json!({}));
    }
    map
}

/// The artifacts of page `key`, made empty when it has none, with every list mesurer wants.
pub fn page_mut<'a>(envelope: &'a mut Value, key: &str) -> &'a mut Map<String, Value> {
    let pages = chrome(envelope)
        .get_mut("pages")
        .and_then(Value::as_object_mut)
        .expect("pages");
    let page = pages.entry(key.to_string()).or_insert(Value::Null);
    if !page.is_object() {
        *page = json!({ "activeMeasurement": null });
    }
    let page = page.as_object_mut().expect("an object");
    for list in PAGE_LISTS {
        if !page.get(list).is_some_and(Value::is_array) {
            page.insert(list.into(), json!([]));
        }
    }
    page
}

/// A comment thread taken out of a page, and where it was, to put it back.
#[derive(Clone, Debug, PartialEq)]
pub struct Taken {
    pub index: usize,
    pub thread: Value,
}

/// Takes thread `id` out of page `key`; `None` when it isn't there.
pub fn take_thread(envelope: &mut Value, key: &str, id: &str) -> Option<Taken> {
    let comments = envelope["pages"][key]["comments"].as_array_mut()?;
    let index = comments.iter().position(|c| c["id"] == id)?;
    Some(Taken {
        index,
        thread: comments.remove(index),
    })
}

/// Puts a taken thread back where it was (at the end, when the page has fewer threads now). A
/// thread of that id already there is left as it is.
pub fn put_thread(envelope: &mut Value, key: &str, taken: Taken) {
    let comments = page_mut(envelope, key)
        .get_mut("comments")
        .and_then(Value::as_array_mut)
        .expect("comments");
    if comments.iter().any(|c| c["id"] == taken.thread["id"]) {
        return;
    }
    let at = taken.index.min(comments.len());
    comments.insert(at, taken.thread);
}

/// `browser_mesurer set`: X-ray, rulers, the tool, minimized (the session's), and the page's
/// guides and layout guides, each replacing what there was. Answers what was set; an argument
/// that isn't one, or a value mesurer wouldn't take, refuses the whole change.
pub fn set(
    envelope: &mut Value,
    key: &str,
    args: &Map<String, Value>,
) -> Result<Vec<String>, String> {
    if let Some(other) = args.keys().find(|k| !SET_KEYS.contains(&k.as_str())) {
        return Err(format!(
            "set doesn't take “{other}”: it takes {}.",
            SET_KEYS.join(", ")
        ));
    }
    if args.is_empty() {
        return Err(format!("Give set one of {}.", SET_KEYS.join(", ")));
    }
    let flag = |name: &str| -> Result<Option<bool>, String> {
        match args.get(name) {
            None => Ok(None),
            Some(Value::Bool(b)) => Ok(Some(*b)),
            Some(other) => Err(format!("{name} is true or false, not {other}.")),
        }
    };
    let (xray, rulers, minimized) = (flag("xray")?, flag("rulers")?, flag("minimized")?);
    let tool = match args.get("tool") {
        None => None,
        Some(Value::String(name)) => {
            let name = name.trim().to_ascii_lowercase();
            let found = TOOLS.iter().find(|(alias, _)| *alias == name);
            Some(found.map(|(_, tool)| *tool).ok_or_else(|| {
                format!(
                    "Unknown tool “{name}”: use none, select (inspect), selection, guides, arrows, \
                     pen, text or comments; X-ray and rulers are set with xray and rulers."
                )
            })?)
        }
        Some(other) => return Err(format!("tool is a name, not {other}.")),
    };
    let guides = args.get("guides").map(guides_of).transpose()?;
    let layout = args
        .get("layout_guides")
        .map(layout_guides_of)
        .transpose()?;

    let chrome = chrome(envelope);
    let mut set = Vec::new();
    if let Some(on) = xray {
        chrome.insert("xrayVisible".into(), on.into());
        set.push("xray".into());
    }
    if let Some(on) = rulers {
        chrome.insert("rulersVisible".into(), on.into());
        set.push("rulers".into());
    }
    if let Some(min) = minimized {
        chrome.insert("minimized".into(), min.into());
        set.push("minimized".into());
    }
    if let Some(tool) = tool {
        chrome.insert("toolMode".into(), tool.into());
        // A tool is used with mesurer on.
        chrome.insert("enabled".into(), true.into());
        set.push(format!("tool {tool}"));
    }
    let page = page_mut(envelope, key);
    if let Some(guides) = guides {
        set.push(format!("{} guides", guides.len()));
        page.insert("guides".into(), Value::Array(guides));
        page.insert("selectedGuideIds".into(), json!([]));
    }
    if let Some(layout) = layout {
        set.push(format!("{} layout guides", layout.len()));
        page.insert("layoutGuides".into(), Value::Array(layout));
    }
    Ok(set)
}

/// `guides`: `[{orientation: vertical|horizontal, position}]`, CSS px from the viewport's left or
/// top, as mesurer's `Guide`s.
fn guides_of(value: &Value) -> Result<Vec<Value>, String> {
    let list = value
        .as_array()
        .ok_or("guides is a list of {orientation, position}.")?;
    list.iter()
        .map(|g| {
            let orientation = match g["orientation"].as_str().unwrap_or("vertical") {
                "vertical" | "v" | "x" => "vertical",
                "horizontal" | "h" | "y" => "horizontal",
                other => return Err(format!("A guide is vertical or horizontal, not “{other}”.")),
            };
            let position = g["position"]
                .as_f64()
                .filter(|p| p.is_finite() && *p >= 0.)
                .ok_or("Each guide needs a position, CSS px from 0.")?;
            Ok(json!({ "id": new_id(), "orientation": orientation, "position": position }))
        })
        .collect()
}

/// `layout_guides`: `[{kind, count, size, gutter, offset, align, color, opacity, visible}]`, each
/// but `kind` optional, as mesurer's `LayoutGuide`s with its defaults (`createLayoutGuide`) and
/// its limits (`normalizeLayoutGuides`).
fn layout_guides_of(value: &Value) -> Result<Vec<Value>, String> {
    let list = value
        .as_array()
        .ok_or("layout_guides is a list of {kind, count, size, gutter, offset, align}.")?;
    list.iter()
        .map(|l| {
            let kind = l["kind"].as_str().unwrap_or("columns");
            if !matches!(kind, "columns" | "rows" | "grid") {
                return Err(format!(
                    "A layout guide's kind is columns, rows or grid, not “{kind}”."
                ));
            }
            let align = l["align"].as_str().unwrap_or("stretch");
            if !matches!(align, "stretch" | "min" | "center" | "max") {
                return Err(format!(
                    "A layout guide's align is stretch, min, center or max, not “{align}”."
                ));
            }
            let number = |name: &str, default: f64, max: f64| -> Result<f64, String> {
                match &l[name] {
                    Value::Null => Ok(default),
                    v => v
                        .as_f64()
                        .filter(|n| n.is_finite())
                        .map(|n| n.clamp(0., max))
                        .ok_or(format!("A layout guide's {name} is a number.")),
                }
            };
            let color = match &l["color"] {
                Value::Null => "#FF0000",
                Value::String(c) if !c.trim().is_empty() && c.len() <= 64 => c.trim(),
                _ => return Err("A layout guide's color is a CSS colour.".into()),
            };
            Ok(json!({
                "id": new_id(),
                "kind": kind,
                "visible": l["visible"].as_bool().unwrap_or(true),
                "color": color,
                "opacity": number("opacity", 0.1, 1.)?,
                "count": number("count", 5., 24.)?.round().max(1.),
                "size": number("size", 72., 4096.)?,
                "gutter": number("gutter", 20., 800.)?,
                "offset": number("offset", 0., 4096.)?,
                "align": align,
            }))
        })
        .collect()
}

/// What `clear` took, by kind.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Cleared {
    pub drawings: usize,
    pub measurements: usize,
    pub guides: usize,
    pub layout_guides: usize,
    /// The comment threads taken, by id.
    pub comments: Vec<String>,
}

/// `browser_mesurer clear`: on page `key`, its drawings (arrows, pen strokes, text notes), its
/// measurements and pinned distances, its guides, its layout guides, its comment threads, or
/// `all` of them.
pub fn clear(envelope: &mut Value, key: &str, what: &str) -> Result<Cleared, String> {
    if !CLEAR_WHAT.contains(&what) {
        return Err(format!(
            "clear takes what: {}; not “{what}”.",
            CLEAR_WHAT.join(", ")
        ));
    }
    let all = what == "all";
    let page = page_mut(envelope, key);
    // Empties the lists; the first one counts, the others are its selections.
    let take = |page: &mut Map<String, Value>, lists: &[&str]| -> usize {
        let mut n = 0;
        for (i, list) in lists.iter().enumerate() {
            if let Some(Value::Array(items)) = page.get_mut(*list) {
                if i == 0 {
                    n += items.len();
                }
                items.clear();
            }
        }
        n
    };
    let mut cleared = Cleared::default();
    if all || what == "drawings" {
        cleared.drawings = take(page, &["arrows", "selectedArrowIds"])
            + take(page, &["penStrokes", "selectedPenStrokeIds"])
            + take(page, &["textAnnotations", "selectedTextIds"]);
    }
    if all || what == "measurements" {
        cleared.measurements = take(page, &["measurements"]) + take(page, &["heldDistances"]);
        page.insert("activeMeasurement".into(), Value::Null);
    }
    if all || what == "guides" {
        cleared.guides = take(page, &["guides", "selectedGuideIds"]);
    }
    if all || what == "layout_guides" {
        cleared.layout_guides = take(page, &["layoutGuides"]);
    }
    if (all || what == "comments")
        && let Some(Value::Array(threads)) = page.get_mut("comments")
    {
        cleared.comments = threads
            .drain(..)
            .filter_map(|t| {
                t["id"]
                    .as_str()
                    .filter(|id| !id.is_empty())
                    .map(str::to_string)
            })
            .collect();
    }
    Ok(cleared)
}
