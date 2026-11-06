//! The terminal tab menu (item 22): its items in their groups, in Title Case, and which ones are
//! on for the first, the last, a locked and a busy tab. Items that don't apply are disabled, not
//! hidden.

use crate::terminal::panel::tab_menu::{MenuState, tab_menu};

fn labels(s: &MenuState) -> Vec<&'static str> {
    tab_menu(s)
        .iter()
        .flat_map(|e| {
            let sep = e.after_separator.then_some("-");
            sep.into_iter().chain([e.label])
        })
        .collect()
}

fn off(s: &MenuState) -> Vec<&'static str> {
    tab_menu(s)
        .into_iter()
        .filter(|e| !e.enabled)
        .map(|e| e.label)
        .collect()
}

/// A tab in the middle of three, in a project, another project open: everything applies.
const MIDDLE: MenuState = MenuState {
    first: false,
    last: false,
    locked: false,
    pinned: false,
    busy: false,
    others: true,
    grouped: true,
    other_projects: true,
};

#[test]
fn the_tab_menu_has_the_file_tab_menus_shape() {
    assert_eq!(
        labels(&MIDDLE),
        [
            "Rename Tab",
            "Duplicate Tab",
            "Move to Project",
            "Remove from Group",
            "-",
            "Close",
            "Close Others",
            "-",
            "Close Left",
            "Close Right",
            "-",
            "Safe Close All",
            "Force Close All",
            "-",
            "Lock Tab",
            "Pin Tab",
            "-",
            "Copy Last Output",
            "Copy Error",
            "Clear",
            "Export Output…",
            "-",
            "Full Screen",
        ]
    );
    assert!(off(&MIDDLE).is_empty());
}

#[test]
fn items_that_do_not_apply_are_off() {
    // The first tab, alone, in no project: nothing left of it, nothing else to close or move to.
    let first = MenuState {
        first: true,
        last: true,
        others: false,
        grouped: false,
        other_projects: false,
        ..MIDDLE
    };
    assert_eq!(
        off(&first),
        [
            "Move to Project",
            "Remove from Group",
            "Close Others",
            "Close Left",
            "Close Right"
        ]
    );
    // The last of several: only Close Right is off.
    let last = MenuState {
        last: true,
        ..MIDDLE
    };
    assert_eq!(off(&last), ["Close Right"]);
    // A locked tab: Close is off, and Lock Tab reads Unlock Tab.
    let locked = MenuState {
        locked: true,
        pinned: true,
        ..MIDDLE
    };
    assert_eq!(off(&locked), ["Close"]);
    let names = labels(&locked);
    assert!(names.contains(&"Unlock Tab") && names.contains(&"Unpin Tab"));
    // A busy tab: everything stays on; the closes ask before they stop what runs.
    let busy = MenuState {
        busy: true,
        ..MIDDLE
    };
    assert!(off(&busy).is_empty());
}
