//! Text an agent writes, revealed at reading speed: about [`WORD`] a word, at most [`MAX_TIME`] a
//! layer, none under reduced motion.
//!
//! An MCP tool call arrives whole, so the text comes at once; the canvas shows it written. The
//! canvas is one bitmap the render thread draws from a document, so what it draws is a copy of the
//! document in which each text being revealed keeps its characters and layout but leaves the
//! words not shown yet unpainted: a style run with no fills over them ([`TextData::restyle`],
//! which FigCraft's renderer paints with nothing). The lines break and align as they will once
//! the text is whole, so nothing moves as words appear. The copy is drawn only, never edited or
//! saved, and made again only when a word more shows.

use std::sync::Arc;
use std::time::{Duration, Instant};

use figcraft_doc::{Document, NodeId, NodeKind};

/// How long each word takes to show.
const WORD: Duration = Duration::from_millis(40);
/// The longest a layer's text takes to show.
const MAX_TIME: Duration = Duration::from_millis(800);
/// How many layers one op may reveal; past it (a large diagram), its text shows whole.
const MAX_LAYERS: usize = 120;

/// One text layer being revealed, word by word.
#[derive(Clone, Debug)]
struct Reveal {
    node: NodeId,
    /// Where each word of the new text ends, in chars from the start of the text.
    ends: Vec<usize>,
    started: Instant,
    /// How long each word takes: [`WORD`], less for a text that would take over [`MAX_TIME`].
    step: Duration,
}

impl Reveal {
    /// How many chars of the text show at `now`; `None` once it all does.
    fn shown(&self, now: Instant) -> Option<usize> {
        let into = now.saturating_duration_since(self.started);
        let words = (into.as_nanos() / self.step.as_nanos().max(1)) as usize + 1;
        (words < self.ends.len()).then(|| self.ends[words - 1])
    }
}

/// A copy of a document drawn while text is revealed: the document it was made from, how much of
/// each text it shows, and the copy.
type Drawn = (Arc<Document>, Vec<Option<usize>>, Arc<Document>);

/// The texts being revealed on a canvas, and the copy of the document last drawn with them.
#[derive(Default)]
pub(in crate::board) struct Reveals {
    texts: Vec<Reveal>,
    drawn: Option<Drawn>,
}

impl Reveals {
    /// The text that an op added or grew from `before` to `after`, revealed from `now`: a new text
    /// layer whole, a text that grew at its end from where it was, any other changed text whole.
    pub fn wrote(&mut self, before: &Document, after: &Document, now: Instant) {
        let mut found = Vec::new();
        for node in after.nodes() {
            let NodeKind::Text(text) = &node.kind else {
                continue;
            };
            let old = before.node(node.id);
            if old.is_some_and(|old| std::ptr::eq(old, node)) {
                continue;
            }
            let old = old
                .and_then(|old| old.kind.text())
                .map(|t| t.characters.as_str());
            let chars = &text.characters;
            if old == Some(chars.as_str()) || chars.trim().is_empty() {
                continue;
            }
            let from = match old {
                Some(old) if chars.starts_with(old) => old.chars().count(),
                _ => 0,
            };
            if let Some(reveal) = reveal(node.id, chars, from, now) {
                found.push(reveal);
            }
            if found.len() > MAX_LAYERS {
                return;
            }
        }
        let ids: Vec<NodeId> = found.iter().map(|r| r.node).collect();
        self.texts.retain(|r| !ids.contains(&r.node));
        self.texts.extend(found);
    }

    /// Whether text is still being revealed at `now`.
    pub fn active(&self, now: Instant) -> bool {
        self.texts.iter().any(|r| r.shown(now).is_some())
    }

    /// The document to draw at `now` in place of `doc`: a copy with the words not shown yet left
    /// unpainted, while text is being revealed; `None` once none is.
    pub fn document(&mut self, doc: &Arc<Document>, now: Instant) -> Option<Arc<Document>> {
        self.texts.retain(|r| r.shown(now).is_some());
        if self.texts.is_empty() {
            self.drawn = None;
            return None;
        }
        let shown: Vec<Option<usize>> = self.texts.iter().map(|r| r.shown(now)).collect();
        if let Some((from, was, copy)) = &self.drawn
            && Arc::ptr_eq(from, doc)
            && *was == shown
        {
            return Some(copy.clone());
        }
        let mut copy = (**doc).clone();
        for (reveal, shown) in self.texts.iter().zip(&shown) {
            let Some(shown) = shown else { continue };
            if let Some(node) = copy.node_mut(reveal.node)
                && let NodeKind::Text(text) = &mut node.kind
            {
                let end = text.characters.chars().count();
                if *shown < end {
                    text.restyle(*shown, end, |style| style.fills = Some(Vec::new()));
                }
            }
        }
        let copy = Arc::new(copy);
        self.drawn = Some((doc.clone(), shown, copy.clone()));
        Some(copy)
    }

    /// The document `doc` was drawn from: itself, or the one a revealing copy was made of.
    pub fn source_of(&self, doc: &Arc<Document>) -> Arc<Document> {
        match &self.drawn {
            Some((from, _, copy)) if Arc::ptr_eq(copy, doc) => from.clone(),
            _ => doc.clone(),
        }
    }
}

/// The reveal of `chars` in layer `node` from char `from` on, word by word from `now`; `None`
/// when there is no word to reveal.
fn reveal(node: NodeId, chars: &str, from: usize, now: Instant) -> Option<Reveal> {
    let mut ends = Vec::new();
    let mut in_word = false;
    for (ix, c) in chars.chars().enumerate().skip(from) {
        match (c.is_whitespace(), in_word) {
            (false, false) => in_word = true,
            (true, true) => {
                ends.push(ix);
                in_word = false;
            }
            _ => {}
        }
    }
    ends.push(chars.chars().count());
    ends.dedup();
    if ends.len() < 2 && from > 0 && ends == [from] {
        return None;
    }
    let words = ends.len() as u32;
    let step = WORD.min(MAX_TIME / words.max(1));
    Some(Reveal {
        node,
        ends,
        started: now,
        step,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn words_show_one_at_a_time_within_the_limit() {
        let now = Instant::now();
        let r = reveal(NodeId(1), "Start free trial", 0, now).unwrap();
        assert_eq!(r.ends, [5, 10, 16]);
        assert_eq!(r.shown(now), Some(5));
        assert_eq!(r.shown(now + WORD), Some(10));
        assert_eq!(r.shown(now + WORD * 2), None, "whole");
        // A long text takes at most MAX_TIME.
        let long = "word ".repeat(100);
        let r = reveal(NodeId(1), &long, 0, now).unwrap();
        assert!(r.shown(now + MAX_TIME).is_none());
        // Appended text reveals from where the text was.
        let r = reveal(NodeId(1), "Pro plan for teams", 8, now).unwrap();
        assert_eq!(r.ends, [12, 18]);
        assert_eq!(r.shown(now), Some(12));
    }
}
