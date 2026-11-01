//! Diagrams in Mermaid syntax as board shapes that can be edited. [`to_skeletons`] reads the SVG
//! that `sawit_core::mermaid::render` draws with Pele and makes [`Skeleton`]s of it for
//! [`crate::skeleton::apply`]: a node becomes a rectangle, an ellipse or a diamond with its label
//! inside, an edge a connector joined to the nodes at its ends, with its label, and whatever else
//! the diagram draws comes in as it is drawn. Charts, whose shapes carry data, come in as one SVG
//! picture.
//!
//! Every type Pele draws (`sawit_core::mermaid::TYPES`) comes onto the board, in one of three
//! ways ([`WAYS`]):
//!
//! | Type | Comes in as | Why |
//! | --- | --- | --- |
//! | flowchart, swimlane, sequence, class, state, er, mindmap, kanban, block, requirement, c4, gitGraph, timeline | shapes | Nodes, edges and boxes: what a board draws. |
//! | architecture | shapes | Groups and services are boxes, links connectors; each icon is a small picture. |
//! | agentflow | shapes | A flowchart of agents: steps, decisions and tools are shapes, flows are clusters. |
//! | usecase | shapes | Actors, use cases (ellipses) and system boundaries, joined by connectors. |
//! | eventmodeling | shapes | Commands, events and views are boxes in lanes, joined by connectors. |
//! | ishikawa | shapes | The head, the categories and each cause are texts and lines the user rewrites. |
//! | packet | shapes | Each field is a box holding its name, its bit numbers texts. |
//! | quadrantChart | shapes | Quadrants are boxes, points are dots with their labels. |
//! | treeView | shapes on a picture | File names are texts; the guides between them a picture under them. |
//! | railroad | shapes on a picture | Terminals and rules are boxes and texts; the tracks, which branch and loop, a picture under them. |
//! | wardley | shapes on a picture | Components, links and moves are shapes; the axes and stages a picture under them. |
//! | cynefin | shapes on a picture | Each domain's names and items are shapes; the regions, curved and see-through, a picture under them. |
//! | venn | shapes on a picture | The labels are texts; the sets, see-through so that their overlaps show, a picture under them. |
//! | journey, pie, gantt, xychart, sankey, radar, treemap, info | one picture | Charts: where a shape sits and how big it is is the data, so moving one would only misstate it. |
//!
//! A picture is an SVG the board imports as its own layers (`file.import`), drawn as Pele drew it.
//! Under "shapes on a picture", what a board shape can't draw (translucent fills, curved regions,
//! branching tracks) is cut from the diagram into one picture, at the back, and the rest comes in
//! as shapes over it ([`BACKDROPS`]). What no shape can hold comes in as a small picture of its own
//! wherever it is: an icon (a nested `<svg>`) and a text Pele turned (a lane's title, an axis's).
//!
//! It is the converter of the web canvas that the native canvas replaced, which read the same SVG
//! laid out in a browser, ported to the SVG's own markup with the same passes: clusters, then
//! nodes, then edges, then what is left. Styles are resolved down the tree as a browser resolves
//! them, transforms are composed with `kurbo`, and a shape's box comes from its geometry. A text
//! has no box until something lays it out, so its place is its `x`/`y` and its `text-anchor`;
//! where a width is needed it is estimated, because the engine sets the text again in its own font.
//!
//! A board holds less than an Excalidraw scene, so some of what that converter kept is left out:
//! line widths and dashes, opacity (the pictures keep both), the colour of the text inside a
//! shape, the weight of text, and groups. Text keeps the size Pele set it in, and a label's room
//! is measured in the face the engine sets it in. A
//! connector has one arrowhead, at its end, so the marks it has no cap for stay drawn where Pele
//! drew them. An edge whose route bends and whose ends join nothing, or one node at both ends (a
//! message to oneself, a loop back to a node), stays drawn along its route.

mod colours;
mod edges;
mod geometry;
mod nodes;
mod pictures;
mod shapes;
mod style;
#[cfg(test)]
mod tests;
mod text;

use std::collections::{HashMap, HashSet};

use kurbo::Rect;
use roxmltree::{Document, Node, NodeId as XmlId};
use sawit_core::mermaid::{Diagram, Palette};

use crate::skeleton::Skeleton;
use colours::Neutrals;
use style::{has_class, is_background, is_icon, is_shape};

/// How a diagram type comes onto the board.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Way {
    /// As shapes, connectors and texts that can be edited.
    Shapes,
    /// As shapes over one picture of what no shape draws ([`BACKDROPS`]).
    ShapesOnPicture,
    /// As one picture of the whole diagram.
    Picture,
}

/// How each type Pele draws (its name for it, `Diagram::kind`) comes onto the board; the module's
/// table says why. A type that isn't here comes in as one picture.
pub const WAYS: &[(&str, Way)] = &[
    ("flowchart", Way::Shapes),
    ("swimlane", Way::Shapes),
    ("sequence", Way::Shapes),
    ("class", Way::Shapes),
    ("state", Way::Shapes),
    ("er", Way::Shapes),
    ("mindmap", Way::Shapes),
    ("kanban", Way::Shapes),
    ("block", Way::Shapes),
    ("requirement", Way::Shapes),
    ("c4", Way::Shapes),
    ("gitGraph", Way::Shapes),
    ("timeline", Way::Shapes),
    ("architecture", Way::Shapes),
    ("agentflow", Way::Shapes),
    ("usecase", Way::Shapes),
    ("eventmodeling", Way::Shapes),
    ("ishikawa", Way::Shapes),
    ("packet", Way::Shapes),
    ("quadrantChart", Way::Shapes),
    ("treeView", Way::ShapesOnPicture),
    ("railroad", Way::ShapesOnPicture),
    ("wardley", Way::ShapesOnPicture),
    ("cynefin", Way::ShapesOnPicture),
    ("venn", Way::ShapesOnPicture),
    ("journey", Way::Picture),
    ("pie", Way::Picture),
    ("gantt", Way::Picture),
    ("xychart", Way::Picture),
    ("sankey", Way::Picture),
    ("radar", Way::Picture),
    ("treemap", Way::Picture),
    ("info", Way::Picture),
];

/// For each type that comes in as shapes on a picture, the classes of what comes in as shapes:
/// those elements and everything in them. Everything else the diagram draws, but the page behind
/// it, is the picture.
pub const BACKDROPS: &[(&str, &[&str])] = &[
    ("treeView", &["pele-node"]),
    ("railroad", &["pele-title", "pele-rule-name", "pele-node"]),
    (
        "wardley",
        &[
            "pele-title",
            "pele-edges",
            "pele-wardley-trends",
            "pele-node",
        ],
    ),
    ("cynefin", &["pele-title", "pele-cynefin-domain"]),
    ("venn", &["pele-title", "pele-venn-labels"]),
];

/// How diagrams of type `kind` (Pele's name for it) come onto the board.
pub fn way_of(kind: &str) -> Way {
    WAYS.iter()
        .find(|(name, _)| *name == kind)
        .map_or(Way::Picture, |(_, way)| *way)
}

/// The diagram types that come in as shapes that can be edited, on a picture or not. Any other
/// comes in as one picture.
pub fn editable(kind: &str) -> bool {
    way_of(kind) != Way::Picture
}

/// The elements Pele draws shapes with.
const SHAPES: &[&str] = &[
    "rect", "circle", "ellipse", "polygon", "polyline", "path", "line",
];
/// The elements an edge's route can be.
const ROUTES: &[&str] = &["path", "line", "polyline"];
/// Elements whose content isn't drawn where it is written.
const NOT_DRAWN: &[&str] = &[
    "defs",
    "clipPath",
    "mask",
    "marker",
    "pattern",
    "symbol",
    "linearGradient",
    "radialGradient",
];

/// The colour that a line or a text in one of Pele's neutral colours comes in as.
const INK: &str = "#1e1e1e";
/// The fill that Pele's second surface colour comes in as.
const SURFACE_ALT_FILL: &str = "#e9ecef";
const TRANSPARENT: &str = "transparent";

/// How far an edge's end may sit from a node's outline and still join it.
const BIND_DISTANCE: f64 = 14.0;
/// How near its route the middle of an edge's label must be to be the connector's own label.
const ON_THE_ROUTE: f64 = 6.0;
/// How far a route may stray from the line between its ends and still run straight.
const STRAIGHT: f64 = 1.0;
/// The straight steps a curve is sampled in.
const CURVE_STEPS: u32 = 8;
/// The font size of a text that names none (the browser's).
const FONT_SIZE: f64 = 16.0;
/// Pele sets a line of text with its baseline this far below the line's middle, in ems.
const BASELINE: f64 = 0.35;
/// The height of a line of text, in ems.
const LINE_HEIGHT: f64 = 1.2;
/// The height of a line of a label, in ems, with room to spare.
const LABEL_LINE_HEIGHT: f64 = 1.25;
/// How much wider than Pele drew it a box may grow to keep a label Pele set on one line on one.
const ONE_LINE_GROWTH: f64 = 1.3;
/// The room around the text of a node that is only text (the web canvas's converter's `pad`).
const TEXT_PAD: f64 = 4.0;
/// The smallest side the engine takes for a shape.
const MIN_SIDE: f64 = 1.0;

/// The shapes that draw `diagram`, in the diagram's own coordinates (0, 0 is its top left).
///
/// A diagram of a type that is [`editable`] comes in as shapes, over a picture of what no shape
/// draws for a type in [`BACKDROPS`]. It is read as rendered with `Palette::default()`, whose
/// neutral colours become the canvas's own: unfilled shapes, ink lines and ink text. The nodes
/// that edges can join get the ids `mermaid-1`, `mermaid-2`..., for the edges to name. Any other
/// type comes in as one `image` skeleton of its SVG, `diagram.svg`, as big as it is drawn.
///
/// The error is for an SVG that can't be read.
pub fn to_skeletons(diagram: &Diagram) -> Result<Vec<Skeleton>, String> {
    if !editable(&diagram.kind) {
        return Ok(vec![picture(diagram)]);
    }
    let svg = Document::parse(&diagram.svg)
        .map_err(|error| format!("the diagram's SVG can't be read ({error})"))?;
    let content = BACKDROPS
        .iter()
        .find(|(kind, _)| *kind == diagram.kind)
        .map(|(_, content)| *content);
    let size = (diagram.width, diagram.height);
    Ok(Converter::new(&svg).run(content, size))
}

/// The whole diagram as one picture.
fn picture(diagram: &Diagram) -> Skeleton {
    Skeleton {
        kind: "image".into(),
        svg: Some(diagram.svg.clone()),
        name: Some("diagram.svg".into()),
        width: Some(diagram.width),
        height: Some(diagram.height),
        ..Skeleton::default()
    }
}

/// A conversion in progress.
struct Converter<'a, 'input> {
    root: Node<'a, 'input>,
    /// The SVG's markup, which the pictures cut from it are made of.
    input: &'input str,
    neutrals: Neutrals,
    /// The groups of the edge labels, by the id of their edge.
    labels: HashMap<&'a str, Node<'a, 'input>>,
    /// The elements already drawn, or drawn as part of something else.
    done: HashSet<XmlId>,
    /// What an edge can join: each node and cluster.
    targets: Vec<Target>,
    /// Whether the diagram has edges, which a node that is only text needs a box for.
    has_edges: bool,
    out: Vec<Skeleton>,
}

/// A node or a cluster that an edge can join.
struct Target {
    /// The id of its skeleton.
    id: String,
    /// Its outline as Pele drew it.
    frame: Rect,
    /// Whether it is a note, which no sequence message ends at.
    note: bool,
}

impl<'a, 'input> Converter<'a, 'input> {
    fn new(svg: &'a Document<'input>) -> Self {
        let root = svg.root_element();
        let mut labels = HashMap::new();
        for group in root
            .descendants()
            .filter(|n| has_class(*n, &["pele-edge-label"]))
        {
            if let Some(id) = group.attribute("data-id") {
                labels.entry(id).or_insert(group);
            }
        }
        Self {
            root,
            input: svg.input_text(),
            neutrals: Neutrals::of(&Palette::default()),
            labels,
            done: HashSet::new(),
            targets: Vec::new(),
            has_edges: root.descendants().any(|n| has_class(n, &["pele-edge"])),
            out: Vec::new(),
        }
    }

    /// The skeletons, back to front. With `content`, the classes of what comes in as shapes (a
    /// type in [`BACKDROPS`]), the rest of the diagram comes first, as one picture as big as the
    /// diagram (`size`).
    fn run(mut self, content: Option<&[&str]>, size: (f64, f64)) -> Vec<Skeleton> {
        // The page that `flatten` paints behind the diagram is no shape of it.
        let first = self.root.children().find(Node::is_element);
        if let Some(background) = first.filter(|n| is_background(*n)) {
            self.done.insert(background.id());
        }
        if let Some(content) = content {
            self.backdrop(content, size);
        }
        // What an icon draws is the icon's picture, which its `<svg>` makes.
        let icons: Vec<Node> = self
            .root
            .descendants()
            .skip(1)
            .filter(|n| is_icon(*n))
            .collect();
        for icon in icons {
            self.done.extend(icon.descendants().skip(1).map(|n| n.id()));
        }
        let elements: Vec<Node> = self
            .root
            .descendants()
            .skip(1)
            .filter(Node::is_element)
            .collect();
        // Clusters first, so that they sit under their nodes; nodes before edges, so that edges
        // can join them.
        let clusters = ["pele-cluster", "pele-frame", "pele-column"];
        for group in elements.iter().filter(|n| has_class(**n, &clusters)) {
            self.group(*group, false);
        }
        let nodes = ["pele-node", "pele-commit", "pele-note"];
        for group in elements.iter().filter(|n| has_class(**n, &nodes)) {
            self.group(*group, true);
        }
        for edge in elements.iter().filter(|n| has_class(**n, &["pele-edge"])) {
            self.edge(*edge);
        }
        for element in elements
            .iter()
            .filter(|n| is_shape(**n) || is_icon(**n) || n.has_tag_name("text"))
        {
            self.loose(*element);
        }
        self.out
    }

    fn is_done(&self, node: Node) -> bool {
        self.done.contains(&node.id())
    }
}
