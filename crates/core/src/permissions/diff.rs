//! A unified diff of two texts, line by line, for showing an edit before it's saved.

/// The diff of `before` and `after` for the file `name`, with three lines of context around
/// each change: `--- name`, `+++ name`, `@@` headers, and ` `, `-`, `+` lines. Empty when they
/// are the same.
pub fn unified(before: &str, after: &str, name: &str) -> String {
    let a: Vec<&str> = before.lines().collect();
    let b: Vec<&str> = after.lines().collect();
    if a == b {
        return String::new();
    }
    // The longest common subsequence, by dynamic programming: settings files are small.
    let (n, m) = (a.len(), b.len());
    let mut lcs = vec![vec![0u32; m + 1]; n + 1];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            lcs[i][j] = if a[i] == b[j] {
                lcs[i + 1][j + 1] + 1
            } else {
                lcs[i + 1][j].max(lcs[i][j + 1])
            };
        }
    }
    // Each line: ' ', '-' or '+', its text, and its line in `a` and `b`.
    let mut ops: Vec<(char, &str)> = Vec::new();
    let (mut i, mut j) = (0, 0);
    while i < n || j < m {
        if i < n && j < m && a[i] == b[j] {
            ops.push((' ', a[i]));
            i += 1;
            j += 1;
        } else if i < n && (j == m || lcs[i + 1][j] >= lcs[i][j + 1]) {
            // What goes, before what takes its place.
            ops.push(('-', a[i]));
            i += 1;
        } else {
            ops.push(('+', b[j]));
            j += 1;
        }
    }
    const CONTEXT: usize = 3;
    let changed: Vec<usize> = (0..ops.len()).filter(|&k| ops[k].0 != ' ').collect();
    let mut out = format!("--- {name}\n+++ {name}\n");
    let mut k = 0;
    while k < changed.len() {
        let start = changed[k].saturating_sub(CONTEXT);
        let mut end = changed[k];
        while k + 1 < changed.len() && changed[k + 1] <= end + 2 * CONTEXT + 1 {
            k += 1;
            end = changed[k];
        }
        let end = (end + CONTEXT + 1).min(ops.len());
        // Line numbers at the hunk's start.
        let a_start = ops[..start].iter().filter(|o| o.0 != '+').count() + 1;
        let b_start = ops[..start].iter().filter(|o| o.0 != '-').count() + 1;
        let a_len = ops[start..end].iter().filter(|o| o.0 != '+').count();
        let b_len = ops[start..end].iter().filter(|o| o.0 != '-').count();
        out.push_str(&format!("@@ -{a_start},{a_len} +{b_start},{b_len} @@\n"));
        for (op, line) in &ops[start..end] {
            out.push(*op);
            out.push_str(line);
            out.push('\n');
        }
        k += 1;
    }
    out
}
