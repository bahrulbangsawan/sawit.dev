//! The commit graph's layout: the lane of each commit, and the edges drawn in its row.

use super::Commit;

/// One line of a drawn edge in a graph row; lanes are columns.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Edge {
    /// Straight through the row in this lane.
    Pass(usize),
    /// From the top of this lane into the row's commit.
    In(usize),
    /// From the row's commit to the bottom of this lane.
    Out(usize),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GraphRow {
    /// The lane the commit's dot is in.
    pub lane: usize,
    pub edges: Vec<Edge>,
}

/// Assigns each commit (newest first, parents after children) a lane and the
/// edges to draw in its row. Lanes keep their column until they end, so a
/// line never jumps sideways except where it meets its commit.
pub fn graph(commits: &[Commit]) -> Vec<GraphRow> {
    // The commit each lane is waiting for.
    let mut lanes: Vec<Option<&str>> = Vec::new();
    commits
        .iter()
        .map(|commit| {
            let mut edges = Vec::new();
            let mut lane = None;
            for (i, waiting) in lanes.iter_mut().enumerate() {
                match *waiting {
                    Some(h) if h == commit.hash => {
                        edges.push(Edge::In(i));
                        lane.get_or_insert(i);
                        *waiting = None;
                    }
                    Some(_) => edges.push(Edge::Pass(i)),
                    None => {}
                }
            }
            let lane = lane.unwrap_or_else(|| free_lane(&mut lanes));
            for (n, parent) in commit.parents.iter().enumerate() {
                let target = if n == 0 {
                    lanes[lane] = Some(parent.as_str());
                    lane
                } else if let Some(i) = lanes.iter().position(|l| *l == Some(parent.as_str())) {
                    i
                } else {
                    let i = free_lane(&mut lanes);
                    lanes[i] = Some(parent.as_str());
                    i
                };
                edges.push(Edge::Out(target));
            }
            while lanes.last() == Some(&None) {
                lanes.pop();
            }
            GraphRow { lane, edges }
        })
        .collect()
}

fn free_lane(lanes: &mut Vec<Option<&str>>) -> usize {
    lanes.iter().position(Option::is_none).unwrap_or_else(|| {
        lanes.push(None);
        lanes.len() - 1
    })
}

/// Lanes a set of rows needs side by side.
pub fn lane_count(rows: &[GraphRow]) -> usize {
    rows.iter()
        .flat_map(|r| {
            std::iter::once(r.lane).chain(r.edges.iter().map(|e| match *e {
                Edge::Pass(l) | Edge::In(l) | Edge::Out(l) => l,
            }))
        })
        .max()
        .map_or(0, |m| m + 1)
}
