//! Which center tabs a close command removes, which tab is active after,
//! and where a tab goes so its project's group stays together. Pure logic,
//! kept apart from the workspace view so it can be tested.

use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloseOp {
    This,
    Others,
    Left,
    Right,
    /// Every tab without unsaved changes.
    Clean,
    All,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TabFlags {
    pub pinned: bool,
    pub dirty: bool,
}

/// Indices (ascending) that `op` closes, relative to `target`. Bulk closes
/// never take pinned tabs or tabs with unsaved changes.
pub fn to_close(op: CloseOp, target: usize, tabs: &[TabFlags]) -> Vec<usize> {
    if target >= tabs.len() {
        return Vec::new();
    }
    if op == CloseOp::This {
        return vec![target];
    }
    tabs.iter()
        .enumerate()
        .filter(|(ix, t)| {
            let in_scope = match op {
                CloseOp::Others => *ix != target,
                CloseOp::Left => *ix < target,
                CloseOp::Right => *ix > target,
                CloseOp::Clean | CloseOp::All | CloseOp::This => true,
            };
            in_scope && !t.pinned && !t.dirty
        })
        .map(|(ix, _)| ix)
        .collect()
}

/// New active index after removing `closed` (ascending) from `len` tabs.
pub fn active_after(active: usize, closed: &[usize], len: usize) -> Option<usize> {
    let survivors: Vec<usize> = (0..len).filter(|ix| !closed.contains(ix)).collect();
    let pick = if survivors.contains(&active) {
        active
    } else {
        survivors
            .iter()
            .rev()
            .find(|&&ix| ix < active)
            .or_else(|| survivors.iter().find(|&&ix| ix > active))
            .copied()?
    };
    survivors.iter().position(|&ix| ix == pick)
}

/// A tab as grouping sees it: pinned, and the project group it is in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Slot<'a> {
    pub pinned: bool,
    pub group: Option<&'a Path>,
}

/// Where a new tab goes. Tabs are kept in the order pinned, ungrouped, then
/// one run per group: a tab joins the end of its run, a new group goes last.
pub fn insert_index(tabs: &[Slot], pinned: bool, group: Option<&Path>) -> usize {
    let pinned_count = tabs.iter().take_while(|t| t.pinned).count();
    if pinned {
        return pinned_count;
    }
    let after_last = |of: Option<&Path>| {
        tabs.iter()
            .rposition(|t| !t.pinned && t.group == of)
            .map(|ix| ix + 1)
    };
    match group {
        Some(_) => after_last(group).unwrap_or(tabs.len()),
        None => after_last(None).unwrap_or(pinned_count),
    }
}

/// Where the active tab ends up when the tab at `from` moves to `to`
/// (`to` counted after the removal).
pub fn active_after_move(active: usize, from: usize, to: usize) -> usize {
    if active == from {
        return to;
    }
    let mut a = active;
    if from < a {
        a -= 1;
    }
    if to <= a {
        a += 1;
    }
    a
}

/// The project holding `path`: the deepest root it is under.
pub fn project_of(roots: &[PathBuf], path: &Path) -> Option<PathBuf> {
    roots
        .iter()
        .filter(|r| path.starts_with(r))
        .max_by_key(|r| r.components().count())
        .cloned()
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use super::{
        CloseOp, Slot, TabFlags, active_after, active_after_move, insert_index, project_of,
        to_close,
    };

    const PLAIN: TabFlags = TabFlags {
        pinned: false,
        dirty: false,
    };
    const DIRTY: TabFlags = TabFlags {
        pinned: false,
        dirty: true,
    };
    const PINNED: TabFlags = TabFlags {
        pinned: true,
        dirty: false,
    };

    #[test]
    fn close_this_closes_only_the_target() {
        assert_eq!(to_close(CloseOp::This, 1, &[PLAIN, PLAIN, PLAIN]), vec![1]);
        // Even a pinned tab, when asked for directly.
        assert_eq!(to_close(CloseOp::This, 0, &[PINNED, PLAIN]), vec![0]);
    }

    #[test]
    fn bulk_closes_keep_pinned_and_unsaved_tabs() {
        let tabs = [PINNED, PLAIN, DIRTY, PLAIN, PLAIN];
        assert_eq!(to_close(CloseOp::Others, 3, &tabs), vec![1, 4]);
        assert_eq!(to_close(CloseOp::Left, 3, &tabs), vec![1]);
        assert_eq!(to_close(CloseOp::Right, 1, &tabs), vec![3, 4]);
        assert_eq!(to_close(CloseOp::All, 0, &tabs), vec![1, 3, 4]);
        assert_eq!(to_close(CloseOp::Clean, 2, &tabs), vec![1, 3, 4]);
    }

    #[test]
    fn out_of_range_target_closes_nothing() {
        assert!(to_close(CloseOp::This, 5, &[PLAIN]).is_empty());
        assert!(to_close(CloseOp::Others, 5, &[PLAIN]).is_empty());
    }

    #[test]
    fn active_tab_follows_the_survivors() {
        // Active tab survives: keep it, shifted left by closed tabs before it.
        assert_eq!(active_after(3, &[0, 1], 5), Some(1));
        // Active tab closed: the nearest survivor to its left, else right.
        assert_eq!(active_after(2, &[2], 4), Some(1));
        assert_eq!(active_after(0, &[0], 3), Some(0));
        assert_eq!(active_after(1, &[0, 1, 2], 4), Some(0));
        // Nothing left.
        assert_eq!(active_after(0, &[0, 1], 2), None);
    }

    fn slot(pinned: bool, group: Option<&str>) -> Slot<'_> {
        Slot {
            pinned,
            group: group.map(Path::new),
        }
    }

    #[test]
    fn tabs_join_their_group_and_new_groups_go_last() {
        let tabs = [
            slot(true, Some("/a")),
            slot(false, None),
            slot(false, Some("/a")),
            slot(false, Some("/a")),
            slot(false, Some("/b")),
        ];
        assert_eq!(insert_index(&tabs, false, Some(Path::new("/a"))), 4);
        assert_eq!(insert_index(&tabs, false, Some(Path::new("/b"))), 5);
        assert_eq!(insert_index(&tabs, false, Some(Path::new("/c"))), 5);
        // Ungrouped tabs sit right after the pinned ones.
        assert_eq!(insert_index(&tabs, false, None), 2);
        assert_eq!(insert_index(&tabs, true, Some(Path::new("/b"))), 1);
        let grouped_only = [slot(true, None), slot(false, Some("/a"))];
        assert_eq!(insert_index(&grouped_only, false, None), 1);
        assert_eq!(insert_index(&[], false, Some(Path::new("/a"))), 0);
    }

    #[test]
    fn active_tab_follows_a_moved_tab() {
        // The active tab is the one moving.
        assert_eq!(active_after_move(1, 1, 3), 3);
        // A tab from before it moves after it, and the other way round.
        assert_eq!(active_after_move(2, 0, 3), 1);
        assert_eq!(active_after_move(1, 3, 0), 2);
        // Moves on one side leave it alone.
        assert_eq!(active_after_move(0, 2, 1), 0);
    }

    #[test]
    fn a_path_belongs_to_the_deepest_project_holding_it() {
        let roots = vec![
            PathBuf::from("/w"),
            PathBuf::from("/w/api"),
            PathBuf::from("/x"),
        ];
        assert_eq!(
            project_of(&roots, Path::new("/w/api/src/main.rs")),
            Some("/w/api".into())
        );
        assert_eq!(
            project_of(&roots, Path::new("/w/web/index.html")),
            Some("/w".into())
        );
        assert_eq!(project_of(&roots, Path::new("/w")), Some("/w".into()));
        // A prefix of the name isn't containment.
        assert_eq!(project_of(&roots, Path::new("/xy/file")), None);
    }
}
