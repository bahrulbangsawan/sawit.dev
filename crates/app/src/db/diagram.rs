//! Schema diagram tab: a schema's tables and foreign keys on a pan-and-zoom
//! canvas (React Flow style). Tables are cards you can drag; foreign keys
//! are curves from the referencing column to the referenced one. Search
//! finds tables and columns and flies to them; a minimap shows where you
//! are. Positions are laid out automatically (`sawit_core::db::diagram::
//! layout`) and saved per schema once you move a table.
//!
//! The same schema is also DBML to read, copy or save, and an SVG (drawn by
//! Pele from a Mermaid erDiagram) to save.

use std::cell::Cell;
use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::*;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use sawit_core::db::diagram::{self, Erd};
use sawit_core::instances::AppPaths;
use sawit_core::mermaid;

use super::store::DbStore;
use crate::rt;
use crate::states::Placeholder;

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
    Pan { last: Point<Pixels>, moved: bool },
    Table { ix: usize, grab: Point<f32>, moved: bool },
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
                InputEvent::PressEnter { shift, .. } => this.step_hit(if *shift { -1 } else { 1 }, cx),
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
            offset: point(px(0.), px(0.)),
            zoom: 1.,
            viewport: Rc::new(Cell::new(Bounds::default())),
            needs_fit: true,
            drag: None,
            selected: None,
            hovered: None,
            search,
            hits: Vec::new(),
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
        let Some(schemas) = self.db.read(cx).schemas.get(&self.profile).cloned() else {
            self.error = Some("Not connected. Connect to the database, then click Reload Schema.".into());
            cx.notify();
            return;
        };
        let only = Some(self.schema.as_str());
        self.error = None;
        self.dbml = diagram::to_dbml(&schemas, only);
        let first = self.erd.tables.is_empty();
        self.erd = diagram::erd(&schemas, only);
        self.sizes = self.erd.tables.iter().map(card_size).collect();
        let saved = load_layouts().remove(&self.layout_key()).unwrap_or_default();
        self.place(&saved);
        self.selected = self.selected.filter(|&s| s < self.erd.tables.len());
        self.hovered = None;
        self.run_search(cx);
        if first {
            self.needs_fit = true;
        }
        cx.notify();
    }

    fn place(&mut self, saved: &HashMap<String, (f32, f32)>) {
        let sizes: Vec<_> = self.sizes.iter().map(|s| (s.width, s.height)).collect();
        let edges: Vec<_> = self.erd.edges.iter().map(|e| (e.from, e.to)).collect();
        let auto = diagram::layout(&sizes, &edges);
        let keys: Vec<String> = self.erd.tables.iter().map(|t| t.key()).collect();
        let below = keys
            .iter()
            .zip(&self.sizes)
            .filter_map(|(k, s)| saved.get(k).map(|p| p.1 + s.height))
            .fold(None, |m: Option<f32>, y| Some(m.map_or(y, |m| m.max(y))));
        // Unsaved tables keep their relative auto layout, shifted below.
        let unsaved_top = keys
            .iter()
            .zip(&auto)
            .filter(|(k, _)| !saved.contains_key(*k))
            .map(|(_, p)| p.1)
            .fold(f32::INFINITY, f32::min);
        self.pos = keys
            .iter()
            .zip(&auto)
            .map(|(k, a)| match (saved.get(k), below) {
                (Some(p), _) => point(p.0, p.1),
                (None, Some(b)) => point(a.0, a.1 - unsaved_top + b + 120.),
                (None, None) => point(a.0, a.1),
            })
            .collect();
    }

    fn layout_key(&self) -> String {
        format!("{}/{}", self.profile, self.schema)
    }

    fn save_layout(&self) {
        let key = self.layout_key();
        let positions: HashMap<String, (f32, f32)> = self
            .erd
            .tables
            .iter()
            .zip(&self.pos)
            .map(|(t, p)| (t.key(), (p.x.round(), p.y.round())))
            .collect();
        std::thread::spawn(move || {
            let mut all = load_layouts();
            all.insert(key, positions);
            if let Ok(json) = serde_json::to_string(&all) {
                let _ = std::fs::write(layouts_file(), json);
            }
        });
    }

    /// Forgets moved positions and lays everything out again.
    fn auto_layout(&mut self, cx: &mut Context<Self>) {
        let key = self.layout_key();
        std::thread::spawn(move || {
            let mut all = load_layouts();
            if all.remove(&key).is_some()
                && let Ok(json) = serde_json::to_string(&all)
            {
                let _ = std::fs::write(layouts_file(), json);
            }
        });
        self.place(&HashMap::new());
        self.fit(cx);
    }

    fn reload(&mut self, cx: &mut Context<Self>) {
        let profile = self.profile.clone();
        // The observer redraws once the tree is back.
        self.db.update(cx, |s, cx| s.refresh(&profile, cx)).detach();
    }

    // Viewport.

    fn to_screen(&self, p: Point<f32>) -> Point<Pixels> {
        let origin = self.viewport.get().origin;
        point(
            origin.x + self.offset.x + px(p.x * self.zoom),
            origin.y + self.offset.y + px(p.y * self.zoom),
        )
    }

    fn to_world(&self, p: Point<Pixels>) -> Point<f32> {
        let origin = self.viewport.get().origin;
        point(
            f32::from(p.x - origin.x - self.offset.x) / self.zoom,
            f32::from(p.y - origin.y - self.offset.y) / self.zoom,
        )
    }

    /// Zooms to `zoom`, keeping the world point under `anchor` (window
    /// coordinates) where it is.
    fn zoom_at(&mut self, zoom: f32, anchor: Point<Pixels>, cx: &mut Context<Self>) {
        let zoom = zoom.clamp(MIN_ZOOM, MAX_ZOOM);
        let world = self.to_world(anchor);
        self.zoom = zoom;
        let origin = self.viewport.get().origin;
        self.offset = point(
            anchor.x - origin.x - px(world.x * zoom),
            anchor.y - origin.y - px(world.y * zoom),
        );
        cx.notify();
    }

    fn zoom_by(&mut self, factor: f32, cx: &mut Context<Self>) {
        let center = self.viewport.get().center();
        self.zoom_at(self.zoom * factor, center, cx);
    }

    /// The world rectangle every table fits in.
    fn world_bounds(&self) -> Option<Bounds<f32>> {
        let mut it = self.pos.iter().zip(&self.sizes);
        let (p, s) = it.next()?;
        let (mut min, mut max) = (*p, point(p.x + s.width, p.y + s.height));
        for (p, s) in it {
            min = point(min.x.min(p.x), min.y.min(p.y));
            max = point(max.x.max(p.x + s.width), max.y.max(p.y + s.height));
        }
        Some(Bounds::from_corners(min, max))
    }

    /// Centers the world point `p` at `zoom`.
    fn center_on(&mut self, p: Point<f32>, zoom: f32) {
        let view = self.viewport.get().size;
        self.zoom = zoom.clamp(MIN_ZOOM, MAX_ZOOM);
        self.offset = point(
            view.width / 2. - px(p.x * self.zoom),
            view.height / 2. - px(p.y * self.zoom),
        );
    }

    fn fit(&mut self, cx: &mut Context<Self>) {
        let view = self.viewport.get().size;
        if let Some(b) = self.world_bounds()
            && view.width > px(0.)
        {
            let pad = 48.;
            let zoom = ((f32::from(view.width) - pad * 2.).max(1.) / b.size.width)
                .min((f32::from(view.height) - pad * 2.).max(1.) / b.size.height)
                .min(1.);
            self.center_on(b.center(), zoom);
            self.needs_fit = false;
        }
        cx.notify();
    }

    /// Flies to a table (and a column row in it).
    fn focus(&mut self, table: usize, column: Option<usize>, cx: &mut Context<Self>) {
        let (p, s) = (self.pos[table], self.sizes[table]);
        let y = match column {
            Some(c) => p.y + HEADER_H + CARD_PAD / 2. + ROW_H * (c as f32 + 0.5),
            None => p.y + s.height / 2.,
        };
        let zoom = if self.zoom < 0.8 { 1. } else { self.zoom };
        self.center_on(point(p.x + s.width / 2., y), zoom);
        self.selected = Some(table);
        cx.notify();
    }

    // Search.

    fn run_search(&mut self, cx: &mut Context<Self>) {
        let query = self.search.read(cx).value();
        self.hits = diagram::search(&self.erd, &query);
        self.hit = 0;
        if let Some(&(t, c)) = self.hits.first() {
            self.focus(t, c, cx);
        } else {
            cx.notify();
        }
    }

    fn step_hit(&mut self, step: isize, cx: &mut Context<Self>) {
        if self.hits.is_empty() {
            return;
        }
        let n = self.hits.len() as isize;
        self.hit = ((self.hit as isize + step).rem_euclid(n)) as usize;
        let (t, c) = self.hits[self.hit];
        self.focus(t, c, cx);
    }

    // Mouse.

    fn on_scroll(&mut self, e: &ScrollEvent, cx: &mut Context<Self>) {
        let delta = e.delta.pixel_delta(px(20.));
        if e.modifiers.platform || e.modifiers.control {
            let factor = (-f32::from(delta.y) * 0.01).exp();
            self.zoom_at(self.zoom * factor, e.position, cx);
        } else {
            self.offset += delta;
            cx.notify();
        }
    }

    fn table_at(&self, p: Point<Pixels>) -> Option<usize> {
        let w = self.to_world(p);
        (0..self.pos.len()).rev().find(|&i| {
            let (o, s) = (self.pos[i], self.sizes[i]);
            w.x >= o.x && w.x <= o.x + s.width && w.y >= o.y && w.y <= o.y + s.height
        })
    }

    fn on_mouse_down(&mut self, e: &MouseDownEvent, cx: &mut Context<Self>) {
        if self.minimap_bounds().is_some_and(|b| b.contains(&e.position)) {
            self.drag = Some(Drag::Minimap);
            self.minimap_jump(e.position, cx);
            return;
        }
        self.drag = Some(match self.table_at(e.position) {
            Some(ix) => {
                let w = self.to_world(e.position);
                Drag::Table {
                    ix,
                    grab: point(w.x - self.pos[ix].x, w.y - self.pos[ix].y),
                    moved: false,
                }
            }
            None => Drag::Pan {
                last: e.position,
                moved: false,
            },
        });
    }

    fn on_mouse_move(&mut self, e: &MouseMoveEvent, cx: &mut Context<Self>) {
        if e.pressed_button != Some(MouseButton::Left) {
            if self.drag.is_some() {
                self.end_drag(cx);
            }
            let hovered = self.table_at(e.position);
            if hovered != self.hovered {
                self.hovered = hovered;
                cx.notify();
            }
            return;
        }
        let w = self.to_world(e.position);
        match &mut self.drag {
            Some(Drag::Pan { last, moved }) => {
                let d = e.position - *last;
                *last = e.position;
                *moved = true;
                self.offset += d;
            }
            Some(Drag::Table { ix, grab, moved }) => {
                *moved = true;
                self.pos[*ix] = point(w.x - grab.x, w.y - grab.y);
            }
            Some(Drag::Minimap) => self.minimap_jump(e.position, cx),
            None => return,
        }
        cx.notify();
    }

    fn end_drag(&mut self, cx: &mut Context<Self>) {
        match self.drag.take() {
            Some(Drag::Table { moved: true, .. }) => self.save_layout(),
            Some(Drag::Table { ix, moved: false, .. }) => {
                self.selected = (self.selected != Some(ix)).then_some(ix);
            }
            Some(Drag::Pan { moved: false, .. }) => self.selected = None,
            _ => {}
        }
        cx.notify();
    }

    // Minimap.

    fn minimap_bounds(&self) -> Option<Bounds<Pixels>> {
        if !self.show_minimap || self.pos.is_empty() {
            return None;
        }
        let v = self.viewport.get();
        let size = size(px(MINIMAP.0), px(MINIMAP.1));
        Some(Bounds::new(
            point(
                v.origin.x + v.size.width - size.width - px(12.),
                v.origin.y + v.size.height - size.height - px(12.),
            ),
            size,
        ))
    }

    /// World bounds the minimap shows (the tables, padded) and its scale.
    fn minimap_frame(&self) -> Option<(Bounds<f32>, f32)> {
        let tables = self.world_bounds()?;
        let pad = 40.;
        let world = Bounds::new(
            point(tables.origin.x - pad, tables.origin.y - pad),
            size(tables.size.width + pad * 2., tables.size.height + pad * 2.),
        );
        let scale = ((MINIMAP.0 - 12.) / world.size.width).min((MINIMAP.1 - 12.) / world.size.height);
        Some((world, scale))
    }

    fn minimap_jump(&mut self, at: Point<Pixels>, cx: &mut Context<Self>) {
        let (Some(b), Some((world, scale))) = (self.minimap_bounds(), self.minimap_frame()) else {
            return;
        };
        let inner = minimap_inner(b, world, scale);
        let p = point(
            world.origin.x + f32::from(at.x - inner.x) / scale,
            world.origin.y + f32::from(at.y - inner.y) / scale,
        );
        self.center_on(p, self.zoom);
        cx.notify();
    }

    // Exports.

    fn copy_dbml(&mut self, cx: &mut Context<Self>) {
        cx.write_to_clipboard(ClipboardItem::new_string(self.dbml.clone()));
        self.notice = Some("DBML copied.".into());
        cx.notify();
    }

    fn export_svg(&mut self, cx: &mut Context<Self>) {
        let Some(schemas) = self.db.read(cx).schemas.get(&self.profile).cloned() else {
            return;
        };
        let source = diagram::to_mermaid(&schemas, Some(&self.schema));
        let palette = crate::editor::palette(cx);
        self.exporting = true;
        cx.notify();
        let rx = rt::spawn_blocking(cx, move || mermaid::render(&source, &palette));
        cx.spawn(async move |this, cx| {
            let result = rx.await;
            let _ = this.update(cx, |this, cx| {
                this.exporting = false;
                match result {
                    Ok(Ok(d)) => this.save("svg", d.svg, cx),
                    Ok(Err(e)) => this.notice = Some(format!("Unable to draw the SVG. {e}")),
                    Err(_) => this.notice = Some("Unable to draw the SVG. Try again.".into()),
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Asks where to save `contents` (default name `<schema>.<ext>`).
    fn save(&mut self, ext: &str, contents: String, cx: &mut Context<Self>) {
        let name = format!("{}.{ext}", self.schema);
        let dir = std::env::var_os("HOME")
            .map(std::path::PathBuf::from)
            .unwrap_or_default();
        let picked = cx.prompt_for_new_path(&dir, Some(&name));
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(path))) = picked.await else {
                return;
            };
            let target = path.clone();
            let Ok(rx) = this.update(cx, |_, cx| {
                rt::spawn(cx, async move { std::fs::write(&target, contents) })
            }) else {
                return;
            };
            let result = rx.await;
            let _ = this.update(cx, |this, cx| {
                let name = path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                this.notice = Some(match result {
                    Ok(Ok(())) => format!("Saved {name}."),
                    Ok(Err(e)) => format!("Unable to save {name}. {e}"),
                    Err(_) => "Unable to save. Try again.".into(),
                });
                cx.notify();
            });
        })
        .detach();
    }

    // Drawing.

    /// Tables related to the selected one, itself included.
    fn focus_set(&self) -> Option<Vec<bool>> {
        let t = self.selected?;
        let mut set = vec![false; self.erd.tables.len()];
        set[t] = true;
        for e in &self.erd.edges {
            if e.from == t {
                set[e.to] = true;
            }
            if e.to == t {
                set[e.from] = true;
            }
        }
        Some(set)
    }

    fn render_card(&self, ix: usize, dim: bool, cx: &App) -> AnyElement {
        let theme = cx.theme();
        let table = &self.erd.tables[ix];
        let z = self.zoom;
        let at = self.to_screen(self.pos[ix]) - self.viewport.get().origin;
        let size = self.sizes[ix];
        let selected = self.selected == Some(ix);
        let current_hit = self.hits.get(self.hit).copied();
        let table_hit = self.hits.contains(&(ix, None));
        let border = if selected {
            theme.primary
        } else if table_hit {
            theme.warning
        } else {
            theme.border
        };
        let detail = z >= DETAIL_ZOOM;
        let text = px((12. * z).max(if detail { 1. } else { 10. }));
        let header = h_flex()
            .h(px(HEADER_H * z))
            .px(px(10. * z))
            .gap(px(6. * z))
            .flex_none()
            .bg(theme.muted)
            .border_b(px(1.))
            .border_color(border.opacity(if selected { 0.6 } else { 1. }))
            .text_size(if detail { text } else { px((14. * z).max(11.)) })
            .child(
                svg()
                    .path(Lucide::Table.path())
                    .size(px(13. * z))
                    .flex_none()
                    .text_color(theme.primary),
            )
            .child(
                div()
                    .min_w_0()
                    .truncate()
                    .font_semibold()
                    .text_color(theme.foreground)
                    .child(table.name.clone()),
            )
            .when(detail, |el| {
                el.child(div().flex_1()).child(
                    div()
                        .flex_none()
                        .text_size(px(10.5 * z))
                        .text_color(theme.muted_foreground)
                        .child(table.schema.clone()),
                )
            });
        let rows = detail.then(|| {
            table.columns.iter().enumerate().map(move |(c, col)| {
                let hit = self.hits.contains(&(ix, Some(c)));
                let current = current_hit == Some((ix, Some(c)));
                let key = if col.pk {
                    Some((Lucide::KeyRound, theme.warning))
                } else if col.fk {
                    Some((Lucide::Link, theme.info))
                } else {
                    None
                };
                h_flex()
                    .h(px(ROW_H * z))
                    .px(px(10. * z))
                    .gap(px(6. * z))
                    .text_size(text)
                    .when(hit, |el| el.bg(theme.warning.opacity(if current { 0.3 } else { 0.14 })))
                    .child(
                        div()
                            .w(px(12. * z))
                            .flex_none()
                            .when_some(key, |el, (icon, color)| {
                                el.child(svg().path(icon.path()).size(px(11. * z)).text_color(color))
                            }),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_color(theme.foreground)
                            .when(col.pk, |el| el.font_semibold())
                            .child(col.name.clone()),
                    )
                    .child(
                        div()
                            .flex_none()
                            .max_w(px(size.width * z * 0.45))
                            .truncate()
                            .font_family(theme.mono_font_family.clone())
                            .text_size(px(11. * z))
                            .text_color(theme.muted_foreground)
                            .child(format!("{}{}", short_type(&col.data_type), if col.nullable { "?" } else { "" })),
                    )
            })
        });
        v_flex()
            .id(("erd-table", ix))
            .absolute()
            .left(at.x)
            .top(at.y)
            .w(px(size.width * z))
            .h(px(size.height * z))
            .overflow_hidden()
            .rounded(px(8. * z))
            .border(px(if selected { 2. } else { 1. }))
            .border_color(border)
            .bg(theme.background)
            .shadow_md()
            .when(dim, |el| el.opacity(0.35))
            .map(|el| {
                if detail {
                    el.child(header)
                } else {
                    // Zoomed out: just the name, as large as fits.
                    let chars = table.name.chars().count().max(4) as f32;
                    let fit = ((size.width * z - 10.) / (chars * 0.6)).clamp(9., 13.);
                    el.bg(theme.muted).child(
                        div()
                            .size_full()
                            .flex()
                            .items_center()
                            .justify_center()
                            .p(px(4.))
                            .text_center()
                            .text_size(px(fit))
                            .line_height(px(fit * 1.2))
                            .font_semibold()
                            .text_color(theme.foreground)
                            .child(table.name.clone()),
                    )
                }
            })
            .when_some(rows, |el, rows| el.pt(px(CARD_PAD * z / 2.)).children(rows))
            .into_any_element()
    }

    fn render_canvas(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        if self.needs_fit {
            if self.viewport.get().size.width > px(0.) {
                self.fit(cx);
            } else {
                window.request_animation_frame();
            }
        }
        let theme = cx.theme();
        let focus = self.focus_set();
        let searching = !self.hits.is_empty();
        let view = self.viewport.get();
        let visible = Bounds::new(point(px(0.), px(0.)), view.size);

        // Edges, in viewport coordinates, painted under the cards.
        let edges: Vec<EdgeShape> = self
            .erd
            .edges
            .iter()
            .map(|e| {
                let active = [self.selected, self.hovered]
                    .iter()
                    .any(|t| *t == Some(e.from) || *t == Some(e.to));
                let row = |t: usize, cols: &[usize]| {
                    let p = self.pos[t];
                    match cols.first() {
                        Some(&c) if self.zoom >= DETAIL_ZOOM => {
                            p.y + HEADER_H + CARD_PAD / 2. + ROW_H * (c as f32 + 0.5)
                        }
                        _ => p.y + HEADER_H / 2.,
                    }
                };
                let (a, b) = (e.from, e.to);
                let (ay, by) = (row(a, &e.from_columns), row(b, &e.to_columns));
                let (pa, sa, pb, sb) = (self.pos[a], self.sizes[a], self.pos[b], self.sizes[b]);
                // Leave from the side facing the other table.
                let (ax, a_dir, bx, b_dir) = if pa.x > pb.x + sb.width {
                    (pa.x, -1., pb.x + sb.width, 1.)
                } else if pb.x > pa.x + sa.width {
                    (pa.x + sa.width, 1., pb.x, -1.)
                } else {
                    // Stacked: loop out on the right.
                    (pa.x + sa.width, 1., pb.x + sb.width, 1.)
                };
                EdgeShape {
                    from: self.to_screen(point(ax, ay)) - view.origin,
                    from_dir: a_dir,
                    to: self.to_screen(point(bx, by)) - view.origin,
                    to_dir: b_dir,
                    optional: e.optional,
                    color: if active {
                        theme.primary
                    } else if focus.is_some() || searching {
                        theme.muted_foreground.opacity(0.25)
                    } else {
                        theme.muted_foreground.opacity(0.6)
                    },
                    width: if active { 2. } else { 1.25 },
                }
            })
            .collect();
        let dot = theme.border;
        let zoom = self.zoom;
        let offset = self.offset;
        let viewport = self.viewport.clone();
        let canvas_layer = canvas(
            move |bounds, _, _| viewport.set(bounds),
            move |bounds, _, window, _| {
                paint_grid(bounds, offset, zoom, dot, window);
                for e in &edges {
                    paint_edge(bounds.origin, e, zoom, window);
                }
            },
        )
        .absolute()
        .size_full();

        let cards: Vec<AnyElement> = (0..self.erd.tables.len())
            .filter(|&i| {
                let at = self.to_screen(self.pos[i]) - view.origin;
                let s = size(px(self.sizes[i].width * zoom), px(self.sizes[i].height * zoom));
                // Before the first paint the viewport is unknown: draw all.
                view.size.width == px(0.) || Bounds::new(at, s).intersects(&visible)
            })
            .map(|i| {
                let dim = match &focus {
                    Some(set) => !set[i],
                    None => searching && !self.hits.iter().any(|h| h.0 == i),
                };
                self.render_card(i, dim, cx)
            })
            .collect();

        let panning = matches!(self.drag, Some(Drag::Pan { moved: true, .. }));
        let moving = matches!(self.drag, Some(Drag::Table { moved: true, .. }));
        div()
            .id("erd-canvas")
            .relative()
            .size_full()
            .overflow_hidden()
            .bg(theme.background)
            .cursor(if panning || moving {
                CursorStyle::ClosedHand
            } else if self.hovered.is_some() {
                CursorStyle::PointingHand
            } else {
                CursorStyle::OpenHand
            })
            .on_scroll(cx.listener(|this, e: &ScrollEvent, _, cx| this.on_scroll(e, cx)))
            .on_pinch(cx.listener(|this, e: &PinchEvent, _, cx| {
                this.zoom_at(this.zoom * (1. + e.delta), e.position, cx)
            }))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, e: &MouseDownEvent, window, cx| {
                    window.blur(cx);
                    this.on_mouse_down(e, cx)
                }),
            )
            .on_mouse_move(cx.listener(|this, e: &MouseMoveEvent, _, cx| this.on_mouse_move(e, cx)))
            .on_mouse_up(MouseButton::Left, cx.listener(|this, _, _, cx| this.end_drag(cx)))
            .on_mouse_up_out(MouseButton::Left, cx.listener(|this, _, _, cx| this.end_drag(cx)))
            .child(canvas_layer)
            .children(cards)
            .child(self.render_controls(cx))
            .when_some(self.render_minimap(cx), |el, m| el.child(m))
            .into_any_element()
    }

    /// Zoom, fit, layout and minimap buttons, bottom-left.
    fn render_controls(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme();
        h_flex()
            .id("erd-controls")
            .absolute()
            .left(px(12.))
            .bottom(px(12.))
            .p(px(3.))
            .gap(px(2.))
            .rounded(px(8.))
            .border_1()
            .border_color(theme.border)
            .bg(theme.popover)
            .shadow_md()
            .text_xs()
            .text_color(theme.muted_foreground)
            // Clicks here shouldn't start a pan underneath.
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(
                Button::new("erd-zoom-out")
                    .ghost()
                    .xsmall()
                    .icon(Lucide::ZoomOut)
                    .tooltip("Zoom Out (⌘ Scroll)")
                    .disabled(self.zoom <= MIN_ZOOM)
                    .on_click(cx.listener(|this, _, _, cx| this.zoom_by(1. / 1.25, cx))),
            )
            .child(
                Button::new("erd-zoom-reset")
                    .ghost()
                    .xsmall()
                    .label(format!("{}%", (self.zoom * 100.).round()))
                    .tooltip("Zoom to 100%")
                    .on_click(cx.listener(|this, _, _, cx| {
                        let z = 1. / this.zoom;
                        this.zoom_by(z, cx)
                    })),
            )
            .child(
                Button::new("erd-zoom-in")
                    .ghost()
                    .xsmall()
                    .icon(Lucide::ZoomIn)
                    .tooltip("Zoom In (⌘ Scroll)")
                    .disabled(self.zoom >= MAX_ZOOM)
                    .on_click(cx.listener(|this, _, _, cx| this.zoom_by(1.25, cx))),
            )
            .child(div().w(px(1.)).h(px(16.)).mx(px(2.)).bg(theme.border))
            .child(
                Button::new("erd-fit")
                    .ghost()
                    .xsmall()
                    .icon(Lucide::Maximize)
                    .tooltip("Fit to Screen")
                    .on_click(cx.listener(|this, _, _, cx| this.fit(cx))),
            )
            .child(
                Button::new("erd-layout")
                    .ghost()
                    .xsmall()
                    .icon(Lucide::LayoutGrid)
                    .tooltip("Auto Layout (forgets moved tables)")
                    .on_click(cx.listener(|this, _, _, cx| this.auto_layout(cx))),
            )
            .child(
                Button::new("erd-minimap")
                    .ghost()
                    .xsmall()
                    .icon(Lucide::Map)
                    .selected(self.show_minimap)
                    .tooltip("Minimap")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.show_minimap = !this.show_minimap;
                        cx.notify();
                    })),
            )
            .into_any_element()
    }

    fn render_minimap(&self, cx: &App) -> Option<AnyElement> {
        let b = self.minimap_bounds()?;
        let (world, scale) = self.minimap_frame()?;
        let theme = cx.theme();
        let view = self.viewport.get();
        let seen = Bounds::from_corners(
            self.to_world(view.origin),
            self.to_world(view.origin + point(view.size.width, view.size.height)),
        );
        let rects: Vec<(Bounds<f32>, bool)> = self
            .pos
            .iter()
            .zip(&self.sizes)
            .enumerate()
            .map(|(i, (p, s))| (Bounds::new(*p, *s), self.selected == Some(i)))
            .collect();
        let (fill_c, sel_c, frame_c) = (theme.muted_foreground.opacity(0.35), theme.primary, theme.primary);
        Some(
            canvas(
                |_, _, _| {},
                move |bounds, _, window, _| {
                    let inner = minimap_inner(bounds, world, scale);
                    let map = |r: Bounds<f32>| {
                        Bounds::new(
                            point(
                                inner.x + px((r.origin.x - world.origin.x) * scale),
                                inner.y + px((r.origin.y - world.origin.y) * scale),
                            ),
                            size(px((r.size.width * scale).max(2.)), px((r.size.height * scale).max(2.))),
                        )
                    };
                    for (r, sel) in &rects {
                        window.paint_quad(fill(map(*r), if *sel { sel_c } else { fill_c }).corner_radii(px(1.5)));
                    }
                    window.paint_quad(outline(map(seen.intersect(&world)), frame_c, BorderStyle::Solid).corner_radii(px(2.)));
                },
            )
            .absolute()
            .left(b.origin.x - view.origin.x)
            .top(b.origin.y - view.origin.y)
            .w(b.size.width)
            .h(b.size.height)
            .rounded(px(8.))
            .border_1()
            .border_color(theme.border)
            .bg(theme.popover)
            .shadow_md()
            .into_any_element(),
        )
    }

    fn render_toolbar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let muted = theme.muted_foreground;
        let tables = self.erd.tables.len();
        let status = if self.exporting {
            "Drawing SVG…".to_string()
        } else if let Some(n) = self.notice.clone() {
            n
        } else {
            let refs = self.erd.edges.len();
            format!(
                "{tables} {} · {refs} {}",
                if tables == 1 { "table" } else { "tables" },
                if refs == 1 { "relation" } else { "relations" }
            )
        };
        let query_empty = self.search.read(cx).value().trim().is_empty();
        let hits = self.hits.len();
        h_flex()
            .px_3()
            .py_1()
            .gap_2()
            .text_xs()
            .text_color(muted)
            .border_b_1()
            .border_color(theme.border)
            .child(
                div()
                    .text_color(theme.foreground)
                    .font_semibold()
                    .child(self.title.clone()),
            )
            .child(div().truncate().child(status))
            .child(div().flex_1())
            .when(!self.show_dbml, |el| {
                el.child(
                    div().w(px(220.)).child(
                        Input::new(&self.search)
                            .xsmall()
                            .prefix(Icon::new(Lucide::Search).xsmall().text_color(muted))
                            .cleanable(true),
                    ),
                )
                .when(!query_empty, |el| {
                    el.child(
                        div()
                            .min_w(px(44.))
                            .text_center()
                            .when(hits == 0, |el| el.text_color(theme.danger))
                            .child(if hits == 0 {
                                "No match".to_string()
                            } else {
                                format!("{}/{hits}", self.hit + 1)
                            }),
                    )
                    .child(
                        Button::new("erd-hit-prev")
                            .ghost()
                            .xsmall()
                            .icon(Lucide::ChevronUp)
                            .tooltip("Previous Match (⇧↩)")
                            .disabled(hits < 2)
                            .on_click(cx.listener(|this, _, _, cx| this.step_hit(-1, cx))),
                    )
                    .child(
                        Button::new("erd-hit-next")
                            .ghost()
                            .xsmall()
                            .icon(Lucide::ChevronDown)
                            .tooltip("Next Match (↩)")
                            .disabled(hits < 2)
                            .on_click(cx.listener(|this, _, _, cx| this.step_hit(1, cx))),
                    )
                })
            })
            .child(
                Button::new("diagram-dbml")
                    .ghost()
                    .xsmall()
                    .label("DBML")
                    .selected(self.show_dbml)
                    .tooltip("Show Schema as DBML")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.show_dbml = !this.show_dbml;
                        cx.notify();
                    })),
            )
            .child(
                Button::new("diagram-copy")
                    .ghost()
                    .xsmall()
                    .icon(Lucide::Copy)
                    .tooltip("Copy DBML")
                    .on_click(cx.listener(|this, _, _, cx| this.copy_dbml(cx))),
            )
            .child(
                Button::new("diagram-save-dbml")
                    .ghost()
                    .xsmall()
                    .icon(Lucide::Download)
                    .label(".dbml")
                    .tooltip("Save DBML…")
                    .on_click(cx.listener(|this, _, _, cx| {
                        let dbml = this.dbml.clone();
                        this.save("dbml", dbml, cx)
                    })),
            )
            .child(
                Button::new("diagram-save-svg")
                    .ghost()
                    .xsmall()
                    .icon(Lucide::Download)
                    .label(".svg")
                    .tooltip("Save Diagram as SVG…")
                    .disabled(tables == 0 || self.exporting)
                    .on_click(cx.listener(|this, _, _, cx| this.export_svg(cx))),
            )
            .child(
                Button::new("diagram-reload")
                    .ghost()
                    .xsmall()
                    .icon(Lucide::RefreshCw)
                    .tooltip("Reload Schema")
                    .on_click(cx.listener(|this, _, _, cx| this.reload(cx))),
            )
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

struct EdgeShape {
    from: Point<Pixels>,
    /// -1 when the edge leaves to the left, 1 to the right.
    from_dir: f32,
    to: Point<Pixels>,
    to_dir: f32,
    optional: bool,
    color: Hsla,
    width: f32,
}

/// One foreign key: a curve with a crow's foot ("many") on the referencing
/// table and a bar ("one") on the referenced one; dashed when optional.
fn paint_edge(origin: Point<Pixels>, e: &EdgeShape, zoom: f32, window: &mut Window) {
    let z = zoom.clamp(0.4, 1.5);
    let (a, b) = (origin + e.from, origin + e.to);
    let stub = px(16. * z);
    let (a1, b1) = (
        point(a.x + stub * e.from_dir, a.y),
        point(b.x + stub * e.to_dir, b.y),
    );
    let reach = ((b1.x - a1.x).abs() / 2.).max(px(40. * z));
    let mut path = PathBuilder::stroke(px(e.width));
    if e.optional {
        path = path.dash_array(&[px(5.), px(4.)]);
    }
    path.move_to(a);
    path.line_to(a1);
    path.cubic_bezier_to(
        b1,
        point(a1.x + reach * e.from_dir, a1.y),
        point(b1.x + reach * e.to_dir, b1.y),
    );
    path.line_to(b);
    if let Ok(path) = path.build() {
        window.paint_path(path, e.color);
    }
    let line = |from: Point<Pixels>, to: Point<Pixels>, window: &mut Window| {
        let mut p = PathBuilder::stroke(px(e.width));
        p.move_to(from);
        p.line_to(to);
        if let Ok(p) = p.build() {
            window.paint_path(p, e.color);
        }
    };
    // Crow's foot: three prongs meeting a short way out from the card.
    let spread = px(6. * z);
    let toe = point(a.x + px(10. * z) * e.from_dir, a.y);
    for dy in [-spread, px(0.), spread] {
        line(toe, point(a.x, a.y + dy), window);
    }
    // One: a bar across the line near the referenced card.
    let bar_x = b.x + px(8. * z) * e.to_dir;
    line(point(bar_x, b.y - spread), point(bar_x, b.y + spread), window);
}

/// A dot grid that moves with the canvas; hidden when too dense.
fn paint_grid(bounds: Bounds<Pixels>, offset: Point<Pixels>, zoom: f32, color: Hsla, window: &mut Window) {
    let mut step = 24. * zoom;
    while step < 12. {
        step *= 2.;
    }
    let step = px(step);
    let r = px(1.);
    let start = |o: Pixels| o - (o / step).floor() * step;
    let mut x = bounds.origin.x + start(offset.x);
    while x < bounds.origin.x + bounds.size.width {
        let mut y = bounds.origin.y + start(offset.y);
        while y < bounds.origin.y + bounds.size.height {
            window.paint_quad(fill(Bounds::centered_at(point(x, y), size(r * 2., r * 2.)), color).corner_radii(r));
            y += step;
        }
        x += step;
    }
}

/// Top-left (in pixels) where the minimap's world frame starts, centered in `b`.
fn minimap_inner(b: Bounds<Pixels>, world: Bounds<f32>, scale: f32) -> Point<Pixels> {
    point(
        b.origin.x + (b.size.width - px(world.size.width * scale)) / 2.,
        b.origin.y + (b.size.height - px(world.size.height * scale)) / 2.,
    )
}

fn card_size(t: &diagram::ErdTable) -> gpui_kit::Size<f32> {
    let header = (t.name.chars().count() + t.schema.chars().count()) as f32 * CHAR_W + 70.;
    let rows = t
        .columns
        .iter()
        .map(|c| (c.name.chars().count() + short_type(&c.data_type).chars().count()) as f32 * CHAR_W + 64.)
        .fold(0., f32::max);
    size(
        header.max(rows).clamp(MIN_W, MAX_W),
        HEADER_H + CARD_PAD + ROW_H * t.columns.len() as f32,
    )
}

/// Shorter spellings of long Postgres type names for the cards.
fn short_type(ty: &str) -> String {
    const SHORT: [(&str, &str); 6] = [
        ("character varying", "varchar"),
        ("timestamp with time zone", "timestamptz"),
        ("timestamp without time zone", "timestamp"),
        ("time with time zone", "timetz"),
        ("time without time zone", "time"),
        ("double precision", "float8"),
    ];
    for (long, short) in SHORT {
        if let Some(rest) = ty.strip_prefix(long) {
            return format!("{short}{rest}");
        }
    }
    ty.to_string()
}

fn layouts_file() -> PathBuf {
    AppPaths::default_macos().root.join("diagram-layouts.json")
}

/// Moved table positions, by `profile/schema` then `schema.table`.
fn load_layouts() -> HashMap<String, HashMap<String, (f32, f32)>> {
    std::fs::read_to_string(layouts_file())
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[::core::prelude::v1::test]
    fn long_types_are_shortened() {
        assert_eq!(short_type("character varying(255)"), "varchar(255)");
        assert_eq!(short_type("timestamp with time zone"), "timestamptz");
        assert_eq!(short_type("bigint"), "bigint");
    }

    #[::core::prelude::v1::test]
    fn cards_grow_with_columns_within_limits() {
        let col = |name: &str| diagram::ErdColumn {
            name: name.into(),
            data_type: "text".into(),
            nullable: false,
            pk: false,
            fk: false,
        };
        let t = diagram::ErdTable {
            schema: "public".into(),
            name: "t".into(),
            columns: vec![col("a"), col(&"x".repeat(200))],
        };
        let s = card_size(&t);
        assert_eq!(s.width, MAX_W);
        assert_eq!(s.height, HEADER_H + CARD_PAD + ROW_H * 2.);
    }
}
