//! Spacing, radius and shadow scales: the values the page uses most, smallest first.

use super::values::{num, parse_box_shadow, px, split_top};
use crate::site_design::scan::Scan;

const SPACING_NAMES: [&str; 7] = ["xs", "sm", "md", "lg", "xl", "2xl", "3xl"];
const RADIUS_NAMES: [&str; 4] = ["sm", "md", "lg", "xl"];
const SHADOW_NAMES: [&str; 3] = ["sm", "md", "lg"];

pub(super) fn derive_spacing(scan: &Scan) -> (Option<f64>, Vec<(String, f64)>) {
    let mut vals: Vec<(f64, u32)> = Vec::new();
    for &(v, n) in &scan.spacing {
        if v <= 0.0 || n == 0 || !v.is_finite() {
            continue;
        }
        match vals.iter_mut().find(|(x, _)| *x == v) {
            Some((_, c)) => *c += n,
            None => vals.push((v, n)),
        }
    }
    let total: u64 = vals.iter().map(|(_, n)| u64::from(*n)).sum();
    if total == 0 {
        return (None, Vec::new());
    }
    let share = |m: f64| -> u64 {
        vals.iter()
            .filter(|(v, _)| (v / m).fract() == 0.0)
            .map(|(_, n)| u64::from(*n))
            .sum()
    };
    let base = if share(4.0) * 10 >= total * 6 {
        Some(4.0)
    } else if share(8.0) * 10 >= total * 6 {
        Some(8.0)
    } else {
        let mut small: Vec<&(f64, u32)> = vals.iter().filter(|(v, _)| *v <= 8.0).collect();
        small.sort_by_key(|a| std::cmp::Reverse(a.1));
        small.first().map(|(v, _)| *v)
    };
    vals.sort_by_key(|a| std::cmp::Reverse(a.1));
    let mut top: Vec<f64> = vals
        .iter()
        .take(SPACING_NAMES.len())
        .map(|(v, _)| *v)
        .collect();
    top.sort_by(|a, b| a.total_cmp(b));
    let scale = SPACING_NAMES
        .iter()
        .zip(top)
        .map(|(n, v)| (n.to_string(), v))
        .collect();
    (base, scale)
}

pub(super) enum Radius {
    Px(f64),
    Full,
}

pub(super) fn parse_radius(s: &str) -> Option<Radius> {
    let first = split_top(s, char::is_whitespace).into_iter().next()?;
    if let Some(p) = first.strip_suffix('%') {
        let p: f64 = p.trim().parse().ok()?;
        return (p >= 50.0).then_some(Radius::Full);
    }
    let v = px(first)?;
    if v >= 500.0 {
        Some(Radius::Full)
    } else {
        Some(Radius::Px(v))
    }
}

pub(super) fn derive_radius(scan: &Scan) -> Vec<(String, String)> {
    let mut full = false;
    let mut vals: Vec<(f64, u32)> = Vec::new();
    for (s, n) in &scan.radii {
        match parse_radius(s) {
            Some(Radius::Full) => full = true,
            Some(Radius::Px(v)) if v > 0.0 => match vals.iter_mut().find(|(x, _)| *x == v) {
                Some((_, c)) => *c += n,
                None => vals.push((v, *n)),
            },
            _ => {}
        }
    }
    vals.sort_by_key(|a| std::cmp::Reverse(a.1));
    let mut top: Vec<f64> = vals
        .iter()
        .take(RADIUS_NAMES.len())
        .map(|(v, _)| *v)
        .collect();
    top.sort_by(|a, b| a.total_cmp(b));
    let mut out: Vec<(String, String)> = RADIUS_NAMES
        .iter()
        .zip(top)
        .map(|(n, v)| (n.to_string(), format!("{}px", num(v, 2))))
        .collect();
    if full {
        out.push(("full".to_string(), "9999px".to_string()));
    }
    out
}

pub(super) fn derive_shadows(scan: &Scan) -> Vec<(String, String)> {
    let mut vals: Vec<(&str, u32)> = Vec::new();
    for (s, n) in &scan.shadows {
        let s = s.trim();
        if s.is_empty() || s.eq_ignore_ascii_case("none") {
            continue;
        }
        match vals.iter_mut().find(|(x, _)| *x == s) {
            Some((_, c)) => *c += n,
            None => vals.push((s, *n)),
        }
    }
    vals.sort_by_key(|a| std::cmp::Reverse(a.1));
    let mut top: Vec<(&str, f64)> = vals
        .iter()
        .take(SHADOW_NAMES.len())
        .map(|(s, _)| (*s, parse_box_shadow(s).map_or(0.0, |x| x.blur)))
        .collect();
    top.sort_by(|a, b| a.1.total_cmp(&b.1));
    SHADOW_NAMES
        .iter()
        .zip(top)
        .map(|(n, (s, _))| (n.to_string(), s.to_string()))
        .collect()
}
