//! The walk's components ([`super::tree`]): an instance as a call to the React component of its
//! component (or its set), each component declared once with its variant props and texts.

use figcraft_doc::{Node, NodeId, NodeKind};
use figcraft_engine::components::{default_variant, variant_options, variant_props};

use super::layers::{ident, overrides};
use super::tree::{Component, Decls, Item, Prop, Walk};

impl Walk<'_> {
    /// Instance `n` of component `main` as a call to its React component, placed by `placed`.
    pub(super) fn call(
        &mut self,
        n: &Node,
        main: NodeId,
        placed: &Decls,
        name: &str,
    ) -> Option<Item> {
        let doc = self.doc.clone();
        let main_node = doc.node(main)?;
        let set = main_node.parent.filter(|p| {
            doc.node(*p)
                .is_some_and(|s| matches!(s.kind, NodeKind::ComponentSet { .. }))
        });
        let key = set.unwrap_or(main);
        let index = match self.by_key.get(&key) {
            Some(index) => *index,
            None => self.component(key, set, main),
        };
        if self.built.insert(main) {
            self.pending.push((index, main));
        }
        let pairs = variant_props(&main_node.name);
        let component = &self.components[index];
        let props = component
            .props
            .iter()
            .filter_map(|p| Some((p.ident.clone(), pairs.get(&p.label)?.clone())))
            .collect();
        let texts = overrides(&doc, n.id, main);
        let class = (!placed.is_empty()).then(|| {
            let class = self.class(&format!("{name} place"), "instance");
            self.placements.push((class.clone(), placed.clone()));
            class
        });
        Some(Item::Call {
            component: index,
            class,
            name: name.to_string(),
            props,
            texts,
        })
    }

    /// Declares the component of `key` (its set, or `main` alone), its default variant to build.
    fn component(&mut self, key: NodeId, set: Option<NodeId>, main: NodeId) -> usize {
        let doc = self.doc.clone();
        let label = doc.node(key).map(|n| n.name.clone()).unwrap_or_default();
        let mut name = crate::screens::component_name(&label);
        while self.components.iter().any(|c| c.name == name) {
            name.push('2');
        }
        let default = set.and_then(|s| default_variant(&doc, s)).unwrap_or(main);
        let defaults = doc
            .node(default)
            .map(|n| variant_props(&n.name))
            .unwrap_or_default();
        let options = set.map(|s| variant_options(&doc, s)).unwrap_or_default();
        let mut idents: Vec<String> = vec!["className".into(), "texts".into()];
        let props = options
            .into_iter()
            .map(|(label, values)| {
                let ident = ident(&label, &idents);
                idents.push(ident.clone());
                let default = defaults
                    .get(&label)
                    .cloned()
                    .unwrap_or_else(|| values[0].clone());
                Prop {
                    ident,
                    label,
                    values,
                    default,
                }
            })
            .collect();
        let mut texts = Vec::new();
        let variants = set
            .and_then(|s| doc.node(s))
            .map(|s| s.children.clone())
            .unwrap_or(vec![main]);
        for variant in variants {
            for id in doc.subtree(variant) {
                if let Some(t) = doc.node(id).filter(|t| matches!(t.kind, NodeKind::Text(_)))
                    && !texts.contains(&t.name)
                {
                    texts.push(t.name.clone());
                }
            }
        }
        self.components.push(Component {
            name,
            props,
            variants: Vec::new(),
            texts,
        });
        let index = self.components.len() - 1;
        self.by_key.insert(key, index);
        if self.built.insert(default) {
            self.pending.push((index, default));
        }
        index
    }
}
