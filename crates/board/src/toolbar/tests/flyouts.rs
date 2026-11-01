//! The flyouts' items: what each runs, and which one is selected.

use super::*;

/// The labels of the items that are selected.
fn selected(items: &[FlyoutItem]) -> Vec<&str> {
    items
        .iter()
        .filter(|item| item.selected)
        .map(|item| item.label.as_str())
        .collect()
}

#[test]
fn a_tools_flyout_lists_its_groups_tools_and_marks_the_one_in_use() {
    for (kind, _) in KINDS {
        let mut doc = empty(kind);
        for group in groups(kind) {
            let Some(Flyout::Tools(key)) = group.flyout else {
                continue;
            };
            assert_eq!(key, group.key);
            let items = flyout(&doc.session, Flyout::Tools(key));
            let tools: Vec<&str> = group.tools.iter().map(|tool| tool.command).collect();
            let listed: Vec<&str> = items.iter().map(|item| item.command).collect();
            let shapes = tools.contains(&"tool.rectangle");
            let mut expected = tools.clone();
            if shapes {
                expected.push(PLACE_IMAGE);
            }
            assert_eq!(listed, expected, "{kind:?} {key}");
            assert!(items.iter().all(|item| item.icon.is_some()), "{key}");
            for item in items.iter().filter(|item| item.command != PLACE_IMAGE) {
                // Picking a tool chooses it, and it is the one marked.
                doc.execute("tool.move", &json!({})).unwrap();
                doc.execute(item.command, &item.params).unwrap();
                let after = flyout(&doc.session, Flyout::Tools(key));
                assert_eq!(selected(&after), [item.label.as_str()], "{key}");
            }
        }
    }
    // Place image… is Sawit's to carry out: it asks for the file.
    assert!(crate::policy::is_ui(PLACE_IMAGE));
}

#[test]
fn sticky_flyout_marks_current_colour() {
    let mut b = board();
    let items = flyout(&b.session, Flyout::StickyColors);
    assert_eq!(items.len(), 8);
    // A new board starts on the first colour.
    assert_eq!(selected(&items), ["yellow"]);

    b.execute("tool.sticky", &json!({"color": "blue"})).unwrap();
    let items = flyout(&b.session, Flyout::StickyColors);
    assert_eq!(items.len(), 8);
    assert_eq!(selected(&items), ["blue"]);
    // Each item sets its colour by name, and shows it.
    let blue = items.iter().find(|item| item.label == "blue").unwrap();
    assert_eq!(blue.command, "tool.sticky");
    assert_eq!(blue.params, json!({"color": "blue"}));
    assert_eq!(blue.swatch, Some([0xAC, 0xD3, 0xFA, 0xFF]));

    // A colour that is none of the eight marks none of them.
    b.execute("tool.sticky", &json!({"color": "#336699"}))
        .unwrap();
    assert_eq!(selected(&flyout(&b.session, Flyout::StickyColors)).len(), 0);
}

#[test]
fn every_flyout_item_runs() {
    let mut b = board();
    let board_groups = groups(DocKind::Board);
    for (f, count) in [
        (Flyout::StickyColors, 8),
        (Flyout::Shapes, 7),
        (Flyout::ConnectorStyles, 3),
        (Flyout::Stamps, 6),
    ] {
        // The tool the flyout belongs to.
        let group = board_groups.iter().find(|group| group.flyout == Some(f));
        let tool = group.and_then(|group| group.tools.first()).unwrap().tool;
        let items = flyout(&b.session, f);
        assert_eq!(items.len(), count, "{f:?}");
        for item in &items {
            // From another tool, so that the item is what makes this one active.
            b.execute("tool.move", &json!({})).unwrap();
            if let Err(e) = b.execute(item.command, &item.params) {
                panic!("{f:?} {}: {e}", item.label);
            }
            assert_eq!(active(&b.session), tool, "{f:?} {}", item.label);
            // It is the choice now, and the only one.
            let after = flyout(&b.session, f);
            assert_eq!(selected(&after), [item.label.as_str()], "{f:?}");
        }
    }
}
