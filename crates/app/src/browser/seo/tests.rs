//! The SEO panel's small rules: how long ago, cut text, points, tab names.

use sawit_core::web::seo::Status;

use super::previews::cut;
use super::rows::{points, status_rank};
use super::{SeoTab, host_of, scanned_ago};

#[test]
fn ago_reads_naturally() {
    assert_eq!(scanned_ago(3), "Scanned just now");
    assert_eq!(scanned_ago(42), "Scanned 42 s ago");
    assert_eq!(scanned_ago(125), "Scanned 2 min ago");
    assert_eq!(scanned_ago(7300), "Scanned 2 h ago");
}

#[test]
fn cut_ends_with_an_ellipsis() {
    assert_eq!(cut("short", 10), "short");
    assert_eq!(cut("a long title here", 8), "a long…");
}

#[test]
fn hosts_keep_their_port() {
    assert_eq!(host_of("http://localhost:3000/a"), "localhost:3000");
    assert_eq!(host_of("https://kyzn.life/"), "kyzn.life");
}

#[test]
fn tabs_by_name_and_ranks() {
    assert_eq!(SeoTab::named("Previews"), Some(SeoTab::Previews));
    assert_eq!(SeoTab::named("agents"), Some(SeoTab::Agents));
    assert_eq!(SeoTab::named("nope"), None);
    assert!(status_rank(Status::Fail) < status_rank(Status::Pass));
}

#[test]
fn points_show_halves_and_states() {
    let mut c = sawit_core::web::seo::CheckResult {
        id: "title".into(),
        name: "Page title".into(),
        layer: sawit_core::web::seo::Layer::Seo,
        tier: sawit_core::web::seo::Tier::Required,
        status: Status::Warn,
        score: 1.5,
        max: 2.,
        details: String::new(),
        evidence: vec![],
        fix: String::new(),
        spec: String::new(),
        seo: true,
        external: false,
    };
    assert_eq!(points(&c), "1.5/2");
    c.status = Status::Na;
    assert_eq!(points(&c), "n/a");
}
