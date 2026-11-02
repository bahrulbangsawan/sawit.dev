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

/// One thing a tab strip draws ([`strip`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Drawn {
    /// The header of the group whose run of tabs starts at tab `first`, `count` tabs long.
    Header { first: usize, count: usize },
    /// The tab at this index.
    Tab(usize),
}

/// What a tab strip draws of `tabs`, in order: a header in front of each group's run of tabs,
/// then the run's tabs, of which a group that is `folded` shows only the active one. Pinned tabs
/// are in no group. The center tab bar and the terminal dock both draw this, one element per
/// item, so the active tab's place in it, not its index, is the child a scroll must bring into
/// view.
pub fn strip(tabs: &[Slot], active: usize, folded: impl Fn(&Path) -> bool) -> Vec<Drawn> {
    let mut drawn = Vec::with_capacity(tabs.len());
    for (ix, tab) in tabs.iter().enumerate() {
        let group = tab.group.filter(|_| !tab.pinned);
        let starts_run = ix == 0 || tabs[ix - 1].pinned || tabs[ix - 1].group != tab.group;
        if group.is_some() && starts_run {
            let count = tabs[ix..]
                .iter()
                .take_while(|t| !t.pinned && t.group == tab.group)
                .count();
            drawn.push(Drawn::Header { first: ix, count });
        }
        if ix != active && group.is_some_and(&folded) {
            continue;
        }
        drawn.push(Drawn::Tab(ix));
    }
    drawn
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

/// Where the active tab is once a tab is inserted at `at` without being shown: the same tab, one
/// further along when the new one went before it. The first tab of all is the active one.
pub fn active_after_insert(active: usize, at: usize, had_tabs: bool) -> usize {
    match had_tabs && at <= active {
        true => active + 1,
        false => active,
    }
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
    /// A tab opened behind the one on show keeps that one on show (an agent's canvas).
    #[test]
    fn a_tab_opened_behind_keeps_the_one_on_show() {
        assert_eq!(super::active_after_insert(2, 5, true), 2);
        assert_eq!(super::active_after_insert(2, 2, true), 3);
        assert_eq!(super::active_after_insert(2, 0, true), 3);
        assert_eq!(
            super::active_after_insert(0, 0, false),
            0,
            "the first tab of all"
        );
    }

    use std::path::{Path, PathBuf};

    use super::{
        CloseOp, Drawn, Slot, TabFlags, active_after, active_after_move, insert_index, project_of,
        strip, to_close,
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

    /// Where the tab at `ix` is among what a strip draws (`drawn`): the child of the strip a
    /// scroll to it goes by.
    fn drawn_at(drawn: &[Drawn], ix: usize) -> Option<usize> {
        drawn.iter().position(|s| *s == Drawn::Tab(ix))
    }

    /// SMOKE-1 and SMOKE-2: a strip draws a header in front of each group's run of tabs and
    /// leaves out a folded group's other tabs, so the active tab's place among what is drawn,
    /// which is what a scroll brings into view, is not its index among the tabs.
    #[test]
    fn a_strip_draws_headers_and_leaves_out_folded_tabs() {
        use Drawn::{Header, Tab};
        let none = |_: &Path| false;
        // Pages of no project (Instance, Browser, Dev Tools...): no header, so the two agree.
        let pages = [slot(false, None), slot(false, None), slot(false, None)];
        assert_eq!(strip(&pages, 2, none), [Tab(0), Tab(1), Tab(2)]);
        assert_eq!(drawn_at(&strip(&pages, 2, none), 2), Some(2));

        // One project, after a page: its header comes before its tabs, so the active tab is
        // child 4, and child 3, which its index names, is the tab left of it.
        let one = [
            slot(false, None),
            slot(false, Some("/p")),
            slot(false, Some("/p")),
            slot(false, Some("/p")),
        ];
        assert_eq!(
            strip(&one, 3, none),
            [
                Tab(0),
                Header { first: 1, count: 3 },
                Tab(1),
                Tab(2),
                Tab(3)
            ]
        );
        assert_eq!(drawn_at(&strip(&one, 3, none), 3), Some(4));

        // Two projects: a header each.
        let two = [
            slot(false, Some("/p")),
            slot(false, Some("/p")),
            slot(false, Some("/q")),
            slot(false, Some("/q")),
        ];
        let drawn = strip(&two, 2, none);
        assert_eq!(
            drawn,
            [
                Header { first: 0, count: 2 },
                Tab(0),
                Tab(1),
                Header { first: 2, count: 2 },
                Tab(2),
                Tab(3),
            ]
        );
        assert_eq!(drawn_at(&drawn, 2), Some(4));
        assert_eq!(drawn_at(&drawn, 3), Some(5));

        // A folded group before the active one draws its header alone, so the active tab's
        // index can be past the last child: here tab 4 is child 3 of 4.
        let folded = |g: &Path| g == Path::new("/p");
        let three_then_two = [
            slot(false, Some("/p")),
            slot(false, Some("/p")),
            slot(false, Some("/p")),
            slot(false, Some("/q")),
            slot(false, Some("/q")),
        ];
        let drawn = strip(&three_then_two, 4, folded);
        assert_eq!(
            drawn,
            [
                Header { first: 0, count: 3 },
                Header { first: 3, count: 2 },
                Tab(3),
                Tab(4),
            ]
        );
        assert_eq!(drawn_at(&drawn, 4), Some(3));
        assert_eq!(drawn_at(&drawn, 0), None, "a folded tab isn't drawn");

        // A folded group still draws its active tab.
        let drawn = strip(&two, 1, folded);
        assert_eq!(
            drawn,
            [
                Header { first: 0, count: 2 },
                Tab(1),
                Header { first: 2, count: 2 },
                Tab(2),
                Tab(3),
            ]
        );
        assert_eq!(drawn_at(&drawn, 1), Some(1));

        // Pinned tabs are in no group, even folded: the header comes after them.
        let pinned = [
            slot(true, Some("/p")),
            slot(true, None),
            slot(false, Some("/p")),
            slot(false, Some("/p")),
        ];
        let drawn = strip(&pinned, 3, folded);
        assert_eq!(
            drawn,
            [Tab(0), Tab(1), Header { first: 2, count: 2 }, Tab(3)]
        );
        assert_eq!(drawn_at(&drawn, 3), Some(3));

        assert!(strip(&[], 0, none).is_empty());
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
