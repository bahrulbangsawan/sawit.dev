//! The document's side of UI screens ([`crate::screens`]): `begin` with `kind: "screen"` marks its
//! frame as a new screen's ([`new_screen`]), and the `screen` op finds a screen by the work that
//! made it or by its name ([`screen`]). A screen's files are its caller's to read and write: the app
//! and `sawit-mcp` know where the canvas file is, and the document doesn't.

use figcraft_doc::{Document, NodeId};
use serde_json::{Value, json};

use super::failure;
use super::work::{work_frame, work_of};
use crate::doc::BoardDoc;
use crate::screens::{Found, ScreenData, Viewport, find, name_for, screen_of, screens};

/// A screen's frame when `begin` names no size: a desktop page.
pub const SCREEN_SIZE: (f64, f64) = (1440.0, 900.0);

/// A new screen called `title` in `doc`, `w` × `h` CSS px: its folder's name, which no screen in
/// `doc` has and none of the op's `taken` (the folders already in the screens folder, which its
/// caller lists), and the value of its frame's plugin data.
pub(super) fn new_screen(
    doc: &Document,
    title: &str,
    op: &Value,
    (w, h): (f64, f64),
) -> (String, String) {
    let mut taken: Vec<String> = screens(doc).into_iter().map(|s| s.data.screen).collect();
    let listed = op
        .get("taken")
        .and_then(Value::as_array)
        .into_iter()
        .flatten();
    taken.extend(listed.filter_map(Value::as_str).map(str::to_string));
    let name = name_for(title, &taken);
    let data = ScreenData {
        screen: name.clone(),
        viewport: Viewport { w, h },
    };
    (name, data.to_json())
}

/// The `screen` op (`{work?, screen?}`): the screen work `work` made, or the one `screen` names (its
/// folder's name, or its frame's name), answered as `{ok, screen, frame, page, name, viewport,
/// made_at}`: `viewport` is the frame's size now, `made_at` the size it was made at. With neither,
/// it lists the screens: `{ok, screens: [...]}`.
pub(super) fn screen(board: &mut BoardDoc, op: &Value) -> Value {
    let doc = match board.doc() {
        Ok(doc) => doc,
        Err(error) => return failure(format!("screen: {error}")),
    };
    let named = op.get("screen").and_then(Value::as_str).map(str::trim);
    let work = match work_of(op) {
        Ok(work) => work,
        Err(error) => return failure(format!("screen: {error}")),
    };
    let found = match (work, named.filter(|n| !n.is_empty())) {
        (Some(work), _) => by_work(&doc, work),
        (None, Some(name)) => by_name(&doc, name),
        (None, None) => {
            let all: Vec<Value> = screens(&doc).iter().map(|s| describe(&doc, s)).collect();
            return json!({"ok": true, "screens": all});
        }
    };
    match found {
        Ok(found) => {
            let mut reply = describe(&doc, &found);
            reply["ok"] = json!(true);
            if let Some(work) = work {
                reply["work"] = json!(work);
            }
            reply
        }
        Err(error) => failure(format!("screen: {error}")),
    }
}

/// The screen work `work`'s frame is.
fn by_work(doc: &Document, work: &str) -> Result<Found, String> {
    let frame = work_frame(doc, work).ok_or_else(|| {
        format!("work “{work}” isn't on the canvas: its frame was deleted, or it was never begun")
    })?;
    let node = doc.node(frame).ok_or("its frame is gone")?;
    let data = screen_of(node).ok_or_else(|| {
        format!(
            "work “{work}” is a component, not a screen: begin one with kind \"screen\" to \
             write HTML"
        )
    })?;
    Ok(Found {
        frame,
        page: doc.page_of(frame).ok_or("its frame is on no page")?,
        data,
    })
}

/// The screen named `name`: by its folder's name, else by its frame's name (any case), else by
/// the folder name that frame name would get.
fn by_name(doc: &Document, name: &str) -> Result<Found, String> {
    if let Some(found) = find(doc, name) {
        return Ok(found);
    }
    let all = screens(doc);
    let titled = all.iter().find(|s| {
        doc.node(s.frame)
            .is_some_and(|node| node.name.trim().eq_ignore_ascii_case(name))
    });
    if let Some(found) = titled {
        return Ok(found.clone());
    }
    if let Some(found) = find(doc, &name_for(name, &[])) {
        return Ok(found);
    }
    let names: Vec<&str> = all.iter().map(|s| s.data.screen.as_str()).collect();
    Err(match names.is_empty() {
        true => format!("no screen is called “{name}”: there is none on the canvas yet"),
        false => format!(
            "no screen is called “{name}”: the canvas has {}",
            names.join(", ")
        ),
    })
}

/// What the `screen` op says of `found`.
fn describe(doc: &Document, found: &Found) -> Value {
    let node = doc.node(found.frame);
    let bounds = doc.world_bounds(found.frame);
    let (w, h) = bounds.map_or((found.data.viewport.w, found.data.viewport.h), |b| {
        ((b.x1 - b.x0).round(), (b.y1 - b.y0).round())
    });
    json!({
        "screen": found.data.screen,
        "frame": found.frame.to_string(),
        "page": found.page.to_string(),
        "name": node.map(|n| n.name.clone()).unwrap_or_default(),
        "viewport": {"w": w, "h": h},
        "made_at": found.data.viewport,
    })
}

/// Runs the export op `op` (`export_svg` or `export_png`) on a copy of `board` in which each
/// frame of `pictures` is filled with its picture (a PNG): the screens' latest snapshots, in
/// place of their empty frames. `board` itself, its undo history and its selection don't change.
pub fn export_with_pictures(board: &BoardDoc, op: &Value, pictures: &[(NodeId, Vec<u8>)]) -> Value {
    use base64::Engine as _;
    let copy = (|| {
        let ds = board.session.ds()?;
        let mut copy = figcraft_engine::DocState::new((*board.document_itself()?).clone(), None);
        copy.page = ds.page;
        let mut session = figcraft_engine::Session::empty();
        session.docs.push(copy);
        Ok::<_, crate::BoardError>(BoardDoc { session })
    })();
    let mut copy = match copy {
        Ok(copy) => copy,
        Err(error) => return failure(error),
    };
    for (frame, png) in pictures {
        let data = base64::engine::general_purpose::STANDARD.encode(png);
        let fill = json!({"data": data, "target": frame.0, "mode": "fill"});
        if let Err(error) = copy.execute("image.place", &fill) {
            log::warn!("a screen's picture couldn't go on frame {frame}: {error}");
        }
    }
    super::run(&mut copy, op)
}
