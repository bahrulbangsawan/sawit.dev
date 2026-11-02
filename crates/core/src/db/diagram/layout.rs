//! Where the ERD canvas puts tables nobody has moved: related tables left to right, groups
//! packed into rows.

const RANK_GAP: f32 = 120.;
const NODE_GAP: f32 = 40.;
const GROUP_GAP: f32 = 120.;

/// Top-left positions for boxes of `sizes` (width, height) so that each
/// group of related tables reads left to right: a referenced table sits in
/// a column left of the tables pointing at it (`edges` are `(from, to)`
/// foreign keys). Groups, then lone tables, are packed into rows.
pub fn layout(sizes: &[(f32, f32)], edges: &[(usize, usize)]) -> Vec<(f32, f32)> {
    let n = sizes.len();
    let mut parents = vec![Vec::new(); n];
    let mut children = vec![Vec::new(); n];
    for &(from, to) in edges {
        if from != to && from < n && to < n && !parents[from].contains(&to) {
            parents[from].push(to);
            children[to].push(from);
        }
    }
    // Connected groups, in first-table order.
    let mut group = vec![usize::MAX; n];
    let mut groups: Vec<Vec<usize>> = Vec::new();
    for start in 0..n {
        if group[start] != usize::MAX {
            continue;
        }
        let g = groups.len();
        let mut members = vec![start];
        group[start] = g;
        let mut i = 0;
        while i < members.len() {
            let t = members[i];
            for &o in parents[t].iter().chain(&children[t]) {
                if group[o] == usize::MAX {
                    group[o] = g;
                    members.push(o);
                }
            }
            i += 1;
        }
        members.sort_unstable();
        groups.push(members);
    }
    // Rank = longest chain of references below a table; cycles are cut
    // where the depth-first walk meets a table it is still inside.
    let mut rank = vec![usize::MAX; n];
    fn rank_of(
        t: usize,
        parents: &[Vec<usize>],
        rank: &mut [usize],
        on_path: &mut [bool],
    ) -> usize {
        if rank[t] != usize::MAX {
            return rank[t];
        }
        on_path[t] = true;
        let mut r = 0;
        for &p in &parents[t] {
            if !on_path[p] {
                r = r.max(rank_of(p, parents, rank, on_path) + 1);
            }
        }
        on_path[t] = false;
        rank[t] = r;
        r
    }
    let mut on_path = vec![false; n];
    for t in 0..n {
        rank_of(t, &parents, &mut rank, &mut on_path);
    }

    // Lay out each group on its own, at (0, 0).
    let mut local = vec![(0f32, 0f32); n];
    let mut boxes: Vec<(usize, f32, f32)> = Vec::new(); // (group, width, height)
    // Each group numbers its own tables before it reads their slots, so one buffer serves all.
    let mut slot = vec![0f32; n];
    for (g, members) in groups.iter().enumerate() {
        let ranks = members.iter().map(|&t| rank[t]).max().unwrap_or(0) + 1;
        let mut cols: Vec<Vec<usize>> = vec![Vec::new(); ranks];
        for &t in members {
            cols[rank[t]].push(t);
        }
        // Order each column by the average slot of its neighbours, a few
        // sweeps each way, so edges cross less.
        let renumber = |cols: &Vec<Vec<usize>>, slot: &mut Vec<f32>| {
            for col in cols {
                for (i, &t) in col.iter().enumerate() {
                    slot[t] = i as f32;
                }
            }
        };
        renumber(&cols, &mut slot);
        for sweep in 0..4 {
            let order: Vec<usize> = if sweep % 2 == 0 {
                (1..ranks).collect()
            } else {
                (0..ranks.saturating_sub(1)).rev().collect()
            };
            for r in order {
                let mut keyed: Vec<(f32, usize)> = cols[r]
                    .iter()
                    .map(|&t| {
                        let near: Vec<f32> = parents[t]
                            .iter()
                            .chain(&children[t])
                            .filter(|&&o| rank[o] + 1 == r || rank[o] == r + 1)
                            .map(|&o| slot[o])
                            .collect();
                        let key = if near.is_empty() {
                            slot[t]
                        } else {
                            near.iter().sum::<f32>() / near.len() as f32
                        };
                        (key, t)
                    })
                    .collect();
                keyed.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
                cols[r] = keyed.into_iter().map(|(_, t)| t).collect();
                renumber(&cols, &mut slot);
            }
        }
        let heights: Vec<f32> = cols
            .iter()
            .map(|c| {
                c.iter().map(|&t| sizes[t].1).sum::<f32>()
                    + NODE_GAP * c.len().saturating_sub(1) as f32
            })
            .collect();
        let tallest = heights.iter().cloned().fold(0., f32::max);
        let mut x = 0.;
        for (r, col) in cols.iter().enumerate() {
            let width = col.iter().map(|&t| sizes[t].0).fold(0., f32::max);
            let mut y = (tallest - heights[r]) / 2.;
            for &t in col {
                local[t] = (x, y);
                y += sizes[t].1 + NODE_GAP;
            }
            x += width + RANK_GAP;
        }
        boxes.push((g, (x - RANK_GAP).max(0.), tallest));
    }

    // Pack groups into rows: bigger groups first, lone tables last.
    let area: f32 = boxes
        .iter()
        .map(|b| (b.1 + GROUP_GAP) * (b.2 + GROUP_GAP))
        .sum();
    let widest = boxes.iter().map(|b| b.1).fold(0., f32::max);
    let row_width = widest.max(area.sqrt() * 1.6);
    boxes.sort_by(|a, b| {
        let lone = |g: usize| groups[g].len() == 1;
        lone(a.0)
            .cmp(&lone(b.0))
            .then(b.2.total_cmp(&a.2))
            .then(a.0.cmp(&b.0))
    });
    let mut out = vec![(0f32, 0f32); n];
    let (mut x, mut y, mut row_h) = (0f32, 0f32, 0f32);
    for (g, w, h) in boxes {
        if x > 0. && x + w > row_width {
            x = 0.;
            y += row_h + GROUP_GAP;
            row_h = 0.;
        }
        for &t in &groups[g] {
            out[t] = (x + local[t].0, y + local[t].1);
        }
        x += w + GROUP_GAP;
        row_h = row_h.max(h);
    }
    out
}
