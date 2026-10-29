//! The Actions palette's model: every command the main menu shows, flattened into a list, and the
//! search over it. The view in `crates/app/src/board/menus.rs` draws the best matches and runs
//! the `id` of the one picked, as a click in the main menu would.
//!
//! The list is the main menu's ([`crate::menu::main_menu`]), so it holds what the menu holds: no
//! hidden command, and no tool (the toolbar has those).

use figcraft_engine::Session;

use crate::menu::{MenuNode, main_menu};

/// What joins the names in an [`ActionItem::path`].
const PATH_SEPARATOR: &str = " › ";

/// One command of the palette.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ActionItem {
    /// The command to run.
    pub id: &'static str,
    pub label: String,
    /// Where the main menu holds it: the menus and the item, joined with ` › `
    /// (`Object › Group selection`).
    pub path: String,
    /// The Mac keys of the command, as the menu writes them (`⌘G`).
    pub shortcut: Option<String>,
}

/// Every enabled item of the main menu of `s`, in menu order.
pub fn all_actions(s: &Session) -> Vec<ActionItem> {
    let mut actions = Vec::new();
    collect(&main_menu(s), "", &mut actions);
    actions
}

/// The `items` that match `query`, best first. Matching ignores case and the space around the
/// query, and looks at the label only. A label matches in three ways, which rank in this order:
/// 1. It starts with the query.
/// 2. A word in it starts with the query. A word starts after anything but a letter or a digit,
///    so `hide` starts a word of `Show/Hide selection`, and so does `to fit` of `Zoom to fit`.
/// 3. The query is a subsequence of it: its characters are in the label, in order.
///
/// Matches of one kind go by the length of the label, shorter first, and then in the order of
/// `items`. An empty query matches everything, in the order of `items`.
pub fn search(items: &[ActionItem], query: &str) -> Vec<ActionItem> {
    let query = query.trim().to_lowercase();
    if query.is_empty() {
        return items.to_vec();
    }
    let mut hits: Vec<(Match, usize, &ActionItem)> = items
        .iter()
        .filter_map(|item| {
            let kind = Match::of(&item.label.to_lowercase(), &query)?;
            Some((kind, item.label.chars().count(), item))
        })
        .collect();
    // The sort is stable: what ties on both keeps the order of `items`.
    hits.sort_by_key(|&(kind, length, _)| (kind, length));
    hits.into_iter().map(|(_, _, item)| item.clone()).collect()
}

/// How a label matches a query, best first: the order the variants are in is the ranking.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Match {
    /// The label starts with the query.
    Prefix,
    /// A word in the label starts with the query.
    Word,
    /// The label holds the query's characters in order.
    Subsequence,
}

impl Match {
    /// How the lower-case `label` matches the lower-case `query`, or `None` if it does not.
    fn of(label: &str, query: &str) -> Option<Self> {
        if label.starts_with(query) {
            Some(Self::Prefix)
        } else if word_starts(label).any(|at| label[at..].starts_with(query)) {
            Some(Self::Word)
        } else if is_subsequence(query, label) {
            Some(Self::Subsequence)
        } else {
            None
        }
    }
}

/// Where the words of `text` start: the byte offsets of the letters and digits that follow
/// something else, or the start of the text.
fn word_starts(text: &str) -> impl Iterator<Item = usize> + '_ {
    let mut after_word = false;
    text.char_indices().filter_map(move |(at, c)| {
        let starts = c.is_alphanumeric() && !after_word;
        after_word = c.is_alphanumeric();
        starts.then_some(at)
    })
}

/// Whether the characters of `needle` are in `haystack`, in order, not always next to each other.
fn is_subsequence(needle: &str, haystack: &str) -> bool {
    let mut rest = haystack.chars();
    needle.chars().all(|wanted| rest.any(|c| c == wanted))
}

/// Add the enabled items among `nodes` to `out`. `parent` is the path of the menu they are in.
fn collect(nodes: &[MenuNode], parent: &str, out: &mut Vec<ActionItem>) {
    for node in nodes {
        match node {
            MenuNode::Item {
                id,
                label,
                shortcut,
                enabled: true,
                ..
            } => out.push(ActionItem {
                id,
                label: label.clone(),
                path: join(parent, label),
                shortcut: shortcut.clone(),
            }),
            MenuNode::Item { .. } | MenuNode::Separator => {}
            MenuNode::Submenu { label, children } => {
                collect(children, &join(parent, label), out);
            }
        }
    }
}

/// `name` below the menu at `parent`, which is empty for a top menu.
fn join(parent: &str, name: &str) -> String {
    if parent.is_empty() {
        name.to_string()
    } else {
        format!("{parent}{PATH_SEPARATOR}{name}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::BoardDoc;
    use crate::menu::{MenuNode, main_menu};

    fn board() -> BoardDoc {
        BoardDoc::new_board("Canvas").unwrap()
    }

    /// The actions of an empty board.
    fn actions() -> Vec<ActionItem> {
        all_actions(&board().session)
    }

    /// An action called `label`. The tests that rank by label need no more of it.
    fn action(label: &str) -> ActionItem {
        ActionItem {
            id: "edit.copy",
            label: label.into(),
            path: format!("Edit › {label}"),
            shortcut: None,
        }
    }

    /// The labels of `items`.
    fn labels(items: &[ActionItem]) -> Vec<&str> {
        items.iter().map(|item| item.label.as_str()).collect()
    }

    /// The ids of the items in `nodes` that can run, in menu order.
    fn enabled_ids(nodes: &[MenuNode]) -> Vec<&'static str> {
        let mut found = Vec::new();
        for node in nodes {
            match node {
                MenuNode::Item {
                    id, enabled: true, ..
                } => found.push(*id),
                MenuNode::Submenu { children, .. } => found.extend(enabled_ids(children)),
                _ => {}
            }
        }
        found
    }

    #[test]
    fn all_actions_are_the_enabled_menu_items() {
        let board = board();
        let ids: Vec<&str> = all_actions(&board.session).iter().map(|a| a.id).collect();
        assert_eq!(ids, enabled_ids(&main_menu(&board.session)));
        assert!(ids.len() > 100, "only {} actions", ids.len());
        // No id twice: running one means one thing.
        let mut sorted = ids.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), ids.len());
    }

    #[test]
    fn actions_carry_their_menu_path_and_shortcut() {
        let actions = actions();
        let find = |id: &str| actions.iter().find(|a| a.id == id).unwrap();
        let group = find("object.group");
        assert_eq!(group.label, "Group selection");
        assert_eq!(group.path, "Object › Group selection");
        assert_eq!(group.shortcut.as_deref(), Some("⌘G"));
        // A submenu's name is part of the path.
        assert_eq!(find("edit.copyAsCss").path, "Edit › Copy as › Copy as CSS");
        assert_eq!(find("help.shortcuts").path, "Help › Keyboard shortcuts");
        assert_eq!(find("view.zoomFit").path, "View › Zoom to fit");
        // Sawit owns ⌘J, and some commands have no keys.
        assert_eq!(find("vector.join").shortcut, None);
        assert_eq!(find("file.revert").shortcut, None);
    }

    #[test]
    fn search_ranks_prefix_first() {
        let all = actions();
        assert!(!all.is_empty());
        assert_eq!(search(&all, "dup")[0].id, "edit.duplicate");
        assert_eq!(search(&all, ""), all);
        assert!(search(&all, "zzz").is_empty());
    }

    #[test]
    fn search_puts_labels_that_start_with_the_query_first() {
        let all = actions();
        let found = search(&all, "layout");
        // "Layout grids" starts with it. "Add auto layout" only has a word that does.
        assert_eq!(found[0].label, "Layout grids");
        assert!(labels(&found).contains(&"Add auto layout"));
        // The word matches come after every prefix match.
        let first_word_match = found
            .iter()
            .position(|a| !a.label.to_lowercase().starts_with("layout"))
            .unwrap();
        assert!(
            found[first_word_match..]
                .iter()
                .all(|a| !a.label.to_lowercase().starts_with("layout")),
            "{:?}",
            labels(&found)
        );
    }

    #[test]
    fn search_ranks_prefix_then_word_then_subsequence() {
        let items = [
            action("Resize to fit"),
            action("Flip it"),
            action("Round"),
            action("Fitting rules"),
            action("Zoom to fit"),
            action("Fit"),
        ];
        // "Flip it" holds f, i, t in that order, and "Round" holds none of them.
        assert_eq!(
            labels(&search(&items, "fit")),
            [
                "Fit",
                "Fitting rules",
                "Zoom to fit",
                "Resize to fit",
                "Flip it"
            ]
        );
    }

    #[test]
    fn search_breaks_ties_by_the_shorter_label() {
        let items = [
            action("Align horizontal centers"),
            action("Align left"),
            action("Align top"),
        ];
        assert_eq!(
            labels(&search(&items, "align")),
            ["Align top", "Align left", "Align horizontal centers"]
        );
        // The same length keeps the menu's order.
        let items = [action("Bold B"), action("Bold A")];
        assert_eq!(labels(&search(&items, "bold")), ["Bold B", "Bold A"]);
    }

    #[test]
    fn search_ranks_by_kind_of_match_before_length() {
        // "to fit" starts a word of "Zoom to fit". "Tot fit" is shorter, but holds the query only
        // as letters in order.
        let items = [action("Tot fit"), action("Zoom to fit")];
        assert_eq!(
            labels(&search(&items, "to fit")),
            ["Zoom to fit", "Tot fit"]
        );
    }

    #[test]
    fn search_ignores_case_and_the_space_around_the_query() {
        let all = actions();
        let plain = search(&all, "dup");
        assert!(!plain.is_empty());
        assert_eq!(search(&all, "DUP"), plain);
        assert_eq!(search(&all, "  Dup "), plain);
        // A query of spaces asks for nothing in particular: everything, in menu order.
        assert_eq!(search(&all, "   "), all);
    }

    #[test]
    fn search_words_start_after_punctuation_too() {
        // "hide" starts the second word of "Show/Hide selection", and "90" of "Rotate 90° left".
        let items = [action("Show/Hide selection"), action("Rotate 90° left")];
        assert_eq!(labels(&search(&items, "hide")), ["Show/Hide selection"]);
        assert_eq!(labels(&search(&items, "90")), ["Rotate 90° left"]);
        // A match in the middle of a word is only letters in order, so it ranks after the others.
        let items = [
            action("Chide"),
            action("Show/Hide selection"),
            action("Hide"),
        ];
        assert_eq!(
            labels(&search(&items, "hide")),
            ["Hide", "Show/Hide selection", "Chide"]
        );
    }
}
