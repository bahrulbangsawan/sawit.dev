//! The SEO scan's tests: robots.txt and sitemap parsing, and whole scans of a loopback site.

use super::robots::{Robots, matches};
use super::sitemap::{Sitemap, llms_shape, markdown_links};
use super::{ScanOptions, ScanType, Status, scan};
use crate::web::tests::serve;

#[test]
fn robots_groups_and_longest_match() {
    let r = Robots::parse(
        "User-agent: GPTBot\nUser-agent: CCBot\nDisallow: /\n\nUser-agent: *\nDisallow: /private\nAllow: /private/ok\nDisallow:\n# c\nSitemap: https://x.dev/s.xml\nContent-Signal: search=yes, ai-train=no\nSchemamap: https://x.dev/schema.xml\n",
    );
    assert_eq!(r.groups.len(), 2);
    assert!(!r.allowed("gptbot", "/a"));
    assert!(!r.allowed("CCBot", "/"));
    assert!(r.allowed("googlebot", "/a"));
    assert!(!r.allowed("googlebot", "/private/x"));
    assert!(r.allowed("googlebot", "/private/ok/1"));
    assert!(r.names("gptbot"));
    assert!(!r.names("googlebot"));
    assert_eq!(r.sitemaps, vec!["https://x.dev/s.xml"]);
    assert_eq!(r.content_signals.len(), 1);
    assert_eq!(r.directive("schemamap"), Some("https://x.dev/schema.xml"));
    assert!(Robots::parse("").allowed("x", "/"));
}

#[test]
fn robots_patterns() {
    assert!(matches("/a*.pdf$", "/a/b.pdf"));
    assert!(!matches("/a*.pdf$", "/a/b.pdf?x"));
    assert!(matches("/a", "/abc"));
    assert!(matches("/$", "/"));
    assert!(!matches("/$", "/x"));
}

#[test]
fn sitemaps_and_llms() {
    let s = Sitemap::parse(
        "<?xml version=\"1.0\"?><urlset><url><loc>https://x.dev/</loc><lastmod>2026-01-01</lastmod></url><url><loc><![CDATA[https://x.dev/a?b=1&amp;c=2]]></loc></url></urlset>",
    )
    .unwrap();
    assert!(!s.index);
    assert_eq!(s.entries[1].0, "https://x.dev/a?b=1&c=2");
    assert_eq!(s.lastmod_share(), 0.5);
    assert!(Sitemap::parse("<html></html>").is_none());
    let i = Sitemap::parse(
        "<sitemapindex><sitemap><loc>https://x.dev/1.xml</loc></sitemap></sitemapindex>",
    )
    .unwrap();
    assert!(i.index);
    let text = "# Site\n\n> Summary\n\n## Docs\n- [A](https://x.dev/a): the a\n- [B](/b)\n";
    let shape = llms_shape(text);
    assert!(shape.h1 && shape.summary);
    assert_eq!((shape.sections, shape.links), (1, 2));
    assert_eq!(markdown_links(text)[1].1, "/b");
}

fn png(w: u32, h: u32) -> Vec<u8> {
    let mut out = Vec::new();
    image::RgbaImage::from_pixel(w, h, image::Rgba([1, 2, 3, 255]))
        .write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png)
        .unwrap();
    out
}

fn good_site() -> String {
    let og = png(1200, 630);
    let icon = png(32, 32);
    let page = r#"<!doctype html><html lang="en"><head>
<title>Good site for testing</title>
<meta name="description" content="A good site that has everything a search engine and a link preview need to show it well.">
<meta name="viewport" content="width=device-width, initial-scale=1">
<link rel="canonical" href="ORIGIN/">
<link rel="icon" href="/icon.png" sizes="32x32">
<link rel="apple-touch-icon" href="/icon.png">
<link rel="manifest" href="/manifest.json">
<meta property="og:title" content="Good"><meta property="og:description" content="Good site">
<meta property="og:image" content="/og.png"><meta property="og:url" content="ORIGIN/"><meta property="og:type" content="website">
<meta name="twitter:card" content="summary_large_image">
<script type="application/ld+json">{"@context":"https://schema.org","@type":"Organization","name":"Good","description":"Good org"}</script>
</head><body><h1>Good</h1><h2>More</h2><img src="/og.png" alt="Share"><a href="/about">About</a><a href="/gone">Gone</a></body></html>"#;
    let origin = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
    let o = origin.clone();
    let base = serve(move |path| {
        let html = |b: String| {
            (
                200,
                vec![("Content-Type".into(), "text/html".into())],
                b.into_bytes(),
            )
        };
        match path {
            "/" => html(page.replace("ORIGIN", &o.lock().unwrap())),
            "/about" => html("<h1>About</h1>".into()),
            "/og.png" => (
                200,
                vec![("Content-Type".into(), "image/png".into())],
                og.clone(),
            ),
            "/icon.png" | "/favicon.ico" => (
                200,
                vec![("Content-Type".into(), "image/png".into())],
                icon.clone(),
            ),
            "/manifest.json" => (
                200,
                vec![("Content-Type".into(), "application/json".into())],
                br#"{"name":"Good","icons":[{"src":"/icon.png"}]}"#.to_vec(),
            ),
            "/robots.txt" => (
                200,
                vec![("Content-Type".into(), "text/plain".into())],
                b"User-agent: *\nAllow: /\nSitemap: /sitemap.xml\n".to_vec(),
            ),
            "/sitemap.xml" => (
                200,
                vec![("Content-Type".into(), "application/xml".into())],
                b"<urlset><url><loc>/</loc></url></urlset>".to_vec(),
            ),
            "/llms.txt" => (
                200,
                vec![("Content-Type".into(), "text/plain".into())],
                b"# Good\n\n> A site\n\n## Pages\n- [About](/about)\n".to_vec(),
            ),
            _ => (
                404,
                vec![("Content-Type".into(), "text/html".into())],
                b"<h1>Not found</h1>".to_vec(),
            ),
        }
    });
    *origin.lock().unwrap() = base.clone();
    base
}

#[test]
fn scan_of_a_good_local_site() {
    let base = good_site();
    let rt = tokio::runtime::Runtime::new().unwrap();
    let report = rt.block_on(scan(&format!("{base}/"), ScanOptions::default()));
    assert!(report.local);
    assert_eq!(report.status, 200);
    let status = |id: &str| report.check(id).unwrap_or_else(|| panic!("{id}")).status;
    for id in [
        "title",
        "meta-description",
        "canonical",
        "noindex-directives",
        "robots-txt",
        "headings",
        "favicon",
        "apple-touch-icon",
        "web-manifest",
        "open-graph",
        "og-image",
        "twitter-card",
        "json-ld",
        "sitemap",
        "llms-txt-exists",
        "llms-txt-formatting",
        "image-alt",
        "viewport",
    ] {
        assert_eq!(status(id), Status::Pass, "{id}: {:?}", report.check(id));
    }
    assert_eq!(status("https"), Status::Na);
    assert_eq!(status("hreflang"), Status::Na);
    assert_eq!(status("llms-full-txt"), Status::Fail);
    assert_eq!(status("broken-links"), Status::Warn);
    let broken = report.check("broken-links").unwrap();
    assert!(broken.evidence[0].url.ends_with("/gone"));
    assert_eq!(broken.evidence[0].status, 404);
    assert_eq!(report.facts.image_width, Some(1200));
    assert_eq!(report.facts.title.as_deref(), Some("Good site for testing"));
    assert!(report.seo_score >= 80, "{}", report.seo_score);
    assert!(!super::fix_prompt_all(&report, true).is_empty());
}

#[test]
fn scan_reads_the_rendered_page_and_filters_checks() {
    let base = good_site();
    let rt = tokio::runtime::Runtime::new().unwrap();
    let report = rt.block_on(scan(
        &format!("{base}/"),
        ScanOptions {
            checks: vec!["title".into(), "headings".into()],
            rendered_html: Some("<title>Rendered title here</title><h1>A</h1><h1>B</h1>".into()),
            scan_type: ScanType::All,
            external: false,
        },
    ));
    assert!(report.rendered);
    assert_eq!(
        report.check("title").unwrap().details,
        "“Rendered title here” (19 characters)."
    );
    assert_eq!(report.check("headings").unwrap().status, Status::Warn);
    assert_eq!(report.check("favicon").unwrap().status, Status::Skipped);
}

#[test]
fn scan_of_nothing_says_why() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let report = rt.block_on(scan("http://127.0.0.1:1/", ScanOptions::default()));
    assert!(report.error.is_some());
    assert_eq!(report.check("title").unwrap().status, Status::Fail);
}

#[test]
fn fix_prompts_name_the_check_and_evidence() {
    let mut c = super::checks::fail("title", "The page has no <title>.");
    c.evidence.push(super::Evidence {
        url: "https://x.dev/".into(),
        status: 200,
        note: String::new(),
    });
    let p = super::fix_prompt("https://x.dev/", &c);
    assert!(p.contains("Page title"));
    assert!(p.contains("https://x.dev/ → 200"));
    assert!(p.contains("How to fix it"));
}

#[test]
fn report_text_lists_problems_first() {
    let base = good_site();
    let rt = tokio::runtime::Runtime::new().unwrap();
    let report = rt.block_on(scan(&format!("{base}/"), ScanOptions::default()));
    let text = super::report_text(&report);
    assert!(text.contains("Search and sharing:"));
    let fix = text.find("To fix:").unwrap();
    let passed = text.find("Passed:").unwrap();
    assert!(fix < passed);
    assert!(text.contains("llms-full-txt"));
}

/// A real scan, printed: `SAWIT_SEO_URL=https://example.com cargo test -p sawit-core live_scan -- --ignored --nocapture`.
#[test]
#[ignore = "fetches a real site"]
fn live_scan() {
    let url = std::env::var("SAWIT_SEO_URL").unwrap_or_else(|_| "https://example.com".into());
    let external = std::env::var("SAWIT_SEO_EXTERNAL").is_ok();
    let rt = tokio::runtime::Runtime::new().unwrap();
    let report = rt.block_on(scan(
        &url,
        ScanOptions {
            external,
            ..Default::default()
        },
    ));
    println!("{}", super::report_text(&report));
    println!("elapsed {} ms", report.elapsed_ms);
    if let Ok(path) = std::env::var("SAWIT_SEO_JSON") {
        std::fs::write(path, serde_json::to_string_pretty(&report).unwrap()).unwrap();
    }
}
