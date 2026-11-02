//! Schema diagram tab: a schema's tables and foreign keys on a pan-and-zoom
//! canvas (React Flow style). Tables are cards you can drag; foreign keys
//! are curves from the referencing column to the referenced one. Search
//! finds tables and columns and flies to them; a minimap shows where you
//! are. Positions are laid out automatically (`sawit_core::db::diagram::
//! layout`) and saved per schema once you move a table.
//!
//! The same schema is also DBML to read, copy or save, and an SVG (drawn by
//! Pele from a Mermaid erDiagram) to save.

mod canvas;
mod card;
mod layout;
mod toolbar;
mod viewport;

use std::cell::Cell;
use std::rc::Rc;

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::*;
use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::component::*;
use gpui_kit::*;
use sawit_core::db::diagram::{self, Erd};

use super::store::DbStore;
use crate::states::Placeholder;
use card::CardText;
use layout::{card_size, load_layouts};

/// Card geometry in world units (pixels at 100%).
const HEADER_H: f32 = 36.;
const ROW_H: f32 = 24.;
const CARD_PAD: f32 = 6.;
const MIN_W: f32 = 200.;
const MAX_W: f32 = 380.;
/// Rough width of one character of the 12px UI and mono fonts.
const CHAR_W: f32 = 7.2;

const MIN_ZOOM: f32 = 0.1;
const MAX_ZOOM: f32 = 2.5;
/// Below this, cards only show their name (columns are unreadable anyway).
const DETAIL_ZOOM: f32 = 0.45;
const MINIMAP: (f32, f32) = (180., 120.);

enum Drag {
    Pan {
        last: Point<Pixels>,
        moved: bool,
    },
    Table {
        ix: usize,
        grab: Point<f32>,
        moved: bool,
    },
    Minimap,
}

pub struct DiagramView {
    pub title: SharedString,
    db: Entity<DbStore>,
    profile: String,
    schema: String,
    dbml: String,
    erd: Erd,
    /// World position (top-left) and size of each table's card.
    pos: Vec<Point<f32>>,
    sizes: Vec<gpui_kit::Size<f32>>,
    /// What each table's card says.
    texts: Vec<CardText>,
    /// Screen offset of the world origin from the viewport's top-left.
    offset: Point<Pixels>,
    zoom: f32,
    /// The canvas's bounds as last painted (window coordinates).
    viewport: Rc<Cell<Bounds<Pixels>>>,
    needs_fit: bool,
    drag: Option<Drag>,
    selected: Option<usize>,
    hovered: Option<usize>,
    search: Entity<InputState>,
    hits: Vec<(usize, Option<usize>)>,
    /// `hits` in order, to look a table or a column up while drawing.
    sorted_hits: Vec<(usize, Option<usize>)>,
    hit: usize,
    show_dbml: bool,
    show_minimap: bool,
    exporting: bool,
    error: Option<String>,
    notice: Option<String>,
    /// `DbStore::schema_rev` this diagram was last checked against.
    schema_rev: u64,
    _subscriptions: Vec<Subscription>,
}

impl DiagramView {
    pub fn new(
        title: SharedString,
        db: Entity<DbStore>,
        profile: String,
        schema: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let search = cx.new(|cx| InputState::new(window, cx).placeholder("Find table or column"));
        let subscriptions = vec![
            // Redraw when the schema tree is reloaded.
            cx.observe(&db, |this: &mut Self, db, cx| {
                // The store notifies for much more than schema reloads
                // (connect state, a 1 Hz retry countdown); only rebuild
                // when they changed.
                let rev = db.read(cx).schema_rev;
                if rev == this.schema_rev {
                    return;
                }
                this.schema_rev = rev;
                let dbml = db
                    .read(cx)
                    .schemas
                    .get(&this.profile)
                    .map(|s| diagram::to_dbml(s, Some(&this.schema)));
                if dbml.is_some_and(|d| d != this.dbml) {
                    this.rebuild(cx);
                }
            }),
            cx.subscribe(&search, |this, _, event: &InputEvent, cx| match event {
                InputEvent::Change => this.run_search(cx),
                InputEvent::PressEnter { shift, .. } => {
                    this.step_hit(if *shift { -1 } else { 1 }, cx)
                }
                _ => {}
            }),
        ];
        let schema_rev = db.read(cx).schema_rev;
        let mut this = Self {
            title,
            db,
            profile,
            schema,
            dbml: String::new(),
            erd: Erd::default(),
            pos: Vec::new(),
            sizes: Vec::new(),
            texts: Vec::new(),
            offset: point(px(0.), px(0.)),
            zoom: 1.,
            viewport: Rc::new(Cell::new(Bounds::default())),
            needs_fit: true,
            drag: None,
            selected: None,
            hovered: None,
            search,
            hits: Vec::new(),
            sorted_hits: Vec::new(),
            hit: 0,
            show_dbml: false,
            show_minimap: true,
            exporting: false,
            error: None,
            notice: None,
            schema_rev,
            _subscriptions: subscriptions,
        };
        this.rebuild(cx);
        this
    }

    /// Reads the schema tree and lays the tables out: saved positions
    /// first, then the automatic layout for tables never moved (placed
    /// below the saved ones).
    fn rebuild(&mut self, cx: &mut Context<Self>) {
        let only = Some(self.schema.as_str());
        // Read where they are: the profile's whole tree isn't copied for one schema.
        let built = self
            .db
            .read(cx)
            .schemas
            .get(&self.profile)
            .map(|schemas| (diagram::to_dbml(schemas, only), diagram::erd(schemas, only)));
        let Some((dbml, erd)) = built else {
            self.error =
                Some("Not connected. Connect to the database, then click Reload Schema.".into());
            cx.notify();
            return;
        };
        self.error = None;
        self.dbml = dbml;
        let first = self.erd.tables.is_empty();
        self.erd = erd;
        self.sizes = self.erd.tables.iter().map(card_size).collect();
        self.texts = self.erd.tables.iter().map(CardText::new).collect();
        let saved = load_layouts()
            .remove(&self.layout_key())
            .unwrap_or_default();
        self.place(&saved);
        self.selected = self.selected.filter(|&s| s < self.erd.tables.len());
        self.hovered = None;
        self.run_search(cx);
        if first {
            self.needs_fit = true;
        }
        cx.notify();
    }

    fn reload(&mut self, cx: &mut Context<Self>) {
        let profile = self.profile.clone();
        // The observer redraws once the tree is back.
        self.db.update(cx, |s, cx| s.refresh(&profile, cx)).detach();
    }
}

impl Render for DiagramView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let body = if self.show_dbml {
            div()
                .id("diagram-dbml-text")
                .size_full()
                .overflow_scroll()
                .p_4()
                .text_sm()
                .font_family(cx.theme().mono_font_family.clone())
                .whitespace_nowrap()
                .children(
                    self.dbml
                        .lines()
                        .map(|l| div().min_h(px(18.)).child(l.to_string()))
                        .collect::<Vec<_>>(),
                )
                .into_any_element()
        } else if let Some(e) = self.error.clone() {
            Placeholder::error("Unable to draw the diagram")
                .detail(e)
                .action(
                    Button::new("diagram-retry")
                        .small()
                        .icon(Lucide::RefreshCw)
                        .label("Reload")
                        .on_click(cx.listener(|this, _, _, cx| this.reload(cx))),
                )
                .into_any_element()
        } else if self.erd.tables.is_empty() {
            Placeholder::new(Lucide::Waypoints, "This schema has no tables")
                .detail("Create a table, then reload to draw its relationships.")
                .into_any_element()
        } else {
            self.render_canvas(window, cx)
        };
        v_flex()
            .size_full()
            .bg(cx.theme().background)
            .child(self.render_toolbar(cx))
            .child(div().flex_1().min_h_0().child(body))
    }
}

#[cfg(test)]
mod tests;
