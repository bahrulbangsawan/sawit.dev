//! A line diff of two small texts (a shell file here and on GitHub), for showing what a sync
//! would change before it does.

/// One line of a diff.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DiffLine {
    Same(String),
    /// Only in the new text.
    Added(String),
    /// Only in the old text.
    Removed(String),
}

/// Beyond this many line pairs the diff says the whole file changed rather than work it out.
const MAX_CELLS: usize = 4_000_000;

/// How `old` becomes `new`, line by line (longest common subsequence).
pub fn line_diff(old: &str, new: &str) -> Vec<DiffLine> {
    let a: Vec<&str> = old.lines().collect();
    let b: Vec<&str> = new.lines().collect();
    // Same start and end need no table.
    let head = a.iter().zip(&b).take_while(|(x, y)| x == y).count();
    let tail = a[head..]
        .iter()
        .rev()
        .zip(b[head..].iter().rev())
        .take_while(|(x, y)| x == y)
        .count();
    let (ma, mb) = (&a[head..a.len() - tail], &b[head..b.len() - tail]);
    let mut out: Vec<DiffLine> = a[..head]
        .iter()
        .map(|l| DiffLine::Same(l.to_string()))
        .collect();
    if ma.len() * mb.len() > MAX_CELLS {
        out.extend(ma.iter().map(|l| DiffLine::Removed(l.to_string())));
        out.extend(mb.iter().map(|l| DiffLine::Added(l.to_string())));
    } else {
        // lcs[i][j]: the common lines of ma[i..] and mb[j..].
        let w = mb.len() + 1;
        let mut lcs = vec![0u32; (ma.len() + 1) * w];
        for i in (0..ma.len()).rev() {
            for j in (0..mb.len()).rev() {
                lcs[i * w + j] = if ma[i] == mb[j] {
                    lcs[(i + 1) * w + j + 1] + 1
                } else {
                    lcs[(i + 1) * w + j].max(lcs[i * w + j + 1])
                };
            }
        }
        let (mut i, mut j) = (0, 0);
        while i < ma.len() || j < mb.len() {
            if i < ma.len() && j < mb.len() && ma[i] == mb[j] {
                out.push(DiffLine::Same(ma[i].to_string()));
                (i, j) = (i + 1, j + 1);
            } else if j < mb.len() && (i == ma.len() || lcs[i * w + j + 1] >= lcs[(i + 1) * w + j])
            {
                out.push(DiffLine::Added(mb[j].to_string()));
                j += 1;
            } else {
                out.push(DiffLine::Removed(ma[i].to_string()));
                i += 1;
            }
        }
    }
    out.extend(
        a[a.len() - tail..]
            .iter()
            .map(|l| DiffLine::Same(l.to_string())),
    );
    out
}

/// Lines added and removed: `(added, removed)`.
pub fn diff_counts(diff: &[DiffLine]) -> (usize, usize) {
    diff.iter().fold((0, 0), |(a, r), l| match l {
        DiffLine::Added(_) => (a + 1, r),
        DiffLine::Removed(_) => (a, r + 1),
        DiffLine::Same(_) => (a, r),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_diff_keeps_the_common_lines_in_order() {
        let diff = line_diff("a\nb\nc\nd\n", "a\nc\nd\ne\n");
        assert_eq!(
            diff,
            [
                DiffLine::Same("a".into()),
                DiffLine::Removed("b".into()),
                DiffLine::Same("c".into()),
                DiffLine::Same("d".into()),
                DiffLine::Added("e".into()),
            ]
        );
        assert_eq!(diff_counts(&diff), (1, 1));
        assert_eq!(diff_counts(&line_diff("x\n", "x\n")), (0, 0));
        assert_eq!(diff_counts(&line_diff("", "1\n2\n")), (2, 0));
    }
}
