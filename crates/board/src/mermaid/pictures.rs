//! The pictures cut from a diagram: the one behind its shapes, and an icon where Pele drew it.

use kurbo::Rect;
use roxmltree::Node;

use super::geometry::shape_box;
use super::style::{escape_attribute, has_class, is_shape, open_tag, tag, visible};
use super::{Converter, MIN_SIDE, NOT_DRAWN};
use crate::skeleton::Skeleton;

impl<'a, 'input> Converter<'a, 'input> {
    // --- Pictures -------------------------------------------------------------------------------

    /// The picture at the back of a type in [`BACKDROPS`]: everything the diagram draws outside the
    /// elements of the `content` classes, which then come in as shapes over it, as big as the
    /// diagram (`size`). Nothing when everything is content.
    pub(super) fn backdrop(&mut self, content: &[&str], size: (f64, f64)) {
        let mut parts = Vec::new();
        let mut stack: Vec<Node> = self.root.children().filter(Node::is_element).collect();
        stack.reverse();
        while let Some(node) = stack.pop() {
            if self.is_done(node) || has_class(node, content) || NOT_DRAWN.contains(&tag(node)) {
                continue;
            }
            let holds_content = node.descendants().skip(1).any(|n| has_class(n, content));
            if !holds_content {
                parts.push(node);
                continue;
            }
            let mut children: Vec<Node> = node.children().filter(Node::is_element).collect();
            children.reverse();
            stack.extend(children);
        }
        let drawn = parts.iter().any(|part| {
            part.descendants()
                .any(|n| (is_shape(n) || n.has_tag_name("text")) && visible(n))
        });
        for part in &parts {
            self.done.extend(part.descendants().map(|n| n.id()));
        }
        if drawn {
            let frame = Rect::new(0.0, 0.0, size.0, size.1);
            let picture = self.cut(&parts, frame, "backdrop.svg");
            self.out.push(picture);
        }
    }

    /// A picture of `parts` (elements of the diagram, each with everything in it) as Pele drew
    /// them, of the diagram's area `frame` and placed there: each part inside the groups around
    /// it, whose transforms and styles it takes, in an SVG with the root's styles and definitions.
    pub(super) fn cut(&self, parts: &[Node], frame: Rect, name: &str) -> Skeleton {
        let (width, height) = (frame.width().max(MIN_SIDE), frame.height().max(MIN_SIDE));
        let mut svg = format!(
            r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="{} {} {width} {height}" width="{width}" height="{height}""#,
            frame.x0, frame.y0
        );
        for attribute in self.root.attributes() {
            let name = attribute.name();
            let kept = attribute.namespace().is_none()
                && !name.starts_with("aria-")
                && !matches!(name, "width" | "height" | "viewBox" | "class" | "role");
            if kept {
                let value = escape_attribute(attribute.value());
                svg.push_str(&format!(r#" {name}="{value}""#));
            }
        }
        svg.push('>');
        // What the parts may refer to: gradients, markers, patterns.
        for defs in self
            .root
            .children()
            .filter(|n| NOT_DRAWN.contains(&tag(*n)))
        {
            svg.push_str(&self.input[defs.range()]);
        }
        for part in parts {
            let around: Vec<Node> = part
                .ancestors()
                .skip(1)
                .filter(|n| n.is_element() && n.parent_element().is_some())
                .collect();
            for group in around.iter().rev() {
                svg.push_str(open_tag(self.input, group.range().start));
            }
            svg.push_str(&self.input[part.range()]);
            for group in &around {
                svg.push_str(&format!("</{}>", tag(*group)));
            }
        }
        svg.push_str("</svg>");
        Skeleton {
            kind: "image".into(),
            x: frame.x0,
            y: frame.y0,
            width: Some(width),
            height: Some(height),
            svg: Some(svg),
            name: Some(name.into()),
            // A frame's title would label every icon and turned text.
            as_group: true,
            ..Skeleton::default()
        }
    }

    /// An icon (a nested `<svg>`) as a picture of itself, where Pele drew it.
    pub(super) fn icon(&mut self, icon: Node<'a, 'input>) -> Option<Skeleton> {
        self.done.insert(icon.id());
        let frame = shape_box(icon).filter(|b| b.width() > 0.0 && b.height() > 0.0)?;
        Some(self.cut(&[icon], frame, "icon.svg"))
    }
}
