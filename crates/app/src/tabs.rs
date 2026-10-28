//! Which center tabs a close command removes, and which tab is active after.
//! Pure logic, kept apart from the workspace view so it can be tested.

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

#[cfg(test)]
mod tests {
    use super::{CloseOp, TabFlags, active_after, to_close};

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
}
