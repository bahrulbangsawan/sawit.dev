//! What the vector tools show while at work (item 11 of [`super::overlay`], with vector edit
//! mode's points): the Shape Builder's regions, each edge light under a thin selection line, and
//! the regions a click or a drag would merge hatched (red when releasing removes them); the
//! lasso; the closed area the paint bucket would fill; and the regions selected or under the
//! pointer in vector edit mode, hatched by the engine.

use figcraft_engine::{DocState, Session, ToolId, shapebuilder};
use kurbo::Affine;

use super::{Rgba, SELECTION, Shape};

const HALO: Rgba = [0xFF, 0xFF, 0xFF, 0x96];
const REMOVE: Rgba = [0xF2, 0x48, 0x22, 0xFF];

fn faded([r, g, b, _]: Rgba, a: u8) -> Rgba {
    [r, g, b, a]
}

pub(super) fn tools(out: &mut Vec<Shape>, s: &Session, ds: &DocState) {
    let view = &ds.view;
    let ov = &s.tool.overlay;
    let screen = |x: f64, y: f64| view.page_to_screen(x, y);
    if s.tool.tool == ToolId::ShapeBuilder
        && let Some(arrangement) = shapebuilder::current(s)
    {
        for region in &arrangement.regions {
            for poly in &region.polys {
                let points: Vec<(f64, f64)> = poly.iter().map(|p| screen(p.x, p.y)).collect();
                if points.len() < 2 {
                    continue;
                }
                for (color, width) in [(HALO, 2.5), (SELECTION, 1.0)] {
                    out.push(Shape::Outline {
                        points: points.clone(),
                        closed: true,
                        color,
                        width,
                        dashed: false,
                    });
                }
            }
        }
        let colour = if ov.builder_remove { REMOVE } else { SELECTION };
        for region in ov
            .builder_regions
            .iter()
            .filter_map(|i| arrangement.regions.get(*i))
        {
            for poly in &region.polys {
                let points: Vec<(f64, f64)> = poly.iter().map(|p| screen(p.x, p.y)).collect();
                if points.len() > 2 {
                    out.push(Shape::Fill {
                        points,
                        color: faded(colour, 0x55),
                    });
                }
            }
        }
    }
    if !ov.face.is_empty() {
        for poly in &ov.face {
            let points: Vec<(f64, f64)> = poly.iter().map(|[x, y]| screen(*x, *y)).collect();
            if points.len() > 2 {
                out.push(Shape::Fill {
                    points,
                    color: faded(SELECTION, 0x40),
                });
            }
        }
    }
    if ov.lasso.len() > 1 {
        out.push(Shape::Outline {
            points: ov.lasso.iter().map(|[x, y]| screen(*x, *y)).collect(),
            closed: true,
            color: SELECTION,
            width: 1.0,
            dashed: true,
        });
    }
    if ds.vector_edit.is_some() {
        let (ox, oy) = view.page_to_screen(0.0, 0.0);
        let z = view.zoom;
        let affine = Affine::new([z, 0.0, 0.0, z, ox, oy]);
        let clip = kurbo::Rect::new(0.0, 0.0, view.vw, view.vh);
        for [a, b] in s.vector_region_hatch(affine, Some(clip)) {
            out.push(Shape::Outline {
                points: vec![(a.x, a.y), (b.x, b.y)],
                closed: false,
                color: SELECTION,
                width: 1.4,
                dashed: false,
            });
        }
    }
}
