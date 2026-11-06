//! ⌘F on the canvas: the layers whose text or name matches, page by page, each page's layers as
//! the layers panel lists them (the top one first, a layer before the layers inside it).

use figcraft_doc::{Document, NodeId};
use sawit_core::find::Regex;

/// The layers on `pages` (in the file's page order) whose text (a text layer's, a sticky's, a
/// shape's label) or name, when it has one of its own, `re` matches.
pub fn find_layers(doc: &Document, pages: &[NodeId], re: &Regex) -> Vec<NodeId> {
    let mut found = Vec::new();
    for page in doc.pages.iter().filter(|p| pages.contains(p)) {
        let Some(page) = doc.node(*page) else {
            continue;
        };
        let mut stack: Vec<NodeId> = page.children.clone();
        while let Some(id) = stack.pop() {
            let Some(node) = doc.node(id) else {
                continue;
            };
            let text = node.kind.text().map(|t| t.characters.as_str());
            if text.is_some_and(|t| re.is_match(t)) || re.is_match(node.name.trim()) {
                found.push(id);
            }
            stack.extend(node.children.iter());
        }
    }
    found
}
