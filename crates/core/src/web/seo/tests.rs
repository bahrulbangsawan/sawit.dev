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

/// A request as the agent-readiness test server sees it.
struct Req {
    method: String,
    path: String,
    headers: Vec<(String, String)>,
    body: String,
}

impl Req {
    fn header(&self, name: &str) -> &str {
        self.headers
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
            .unwrap_or("")
    }
}

type Reply = (u16, Vec<(&'static str, String)>, Vec<u8>);

/// A loopback server that reads the method, headers and body.
fn serve_full(reply: impl Fn(&Req) -> Reply + Send + Sync + 'static) -> String {
    use std::io::{BufRead, BufReader, Read, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let reply = std::sync::Arc::new(reply);
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let reply = reply.clone();
            std::thread::spawn(move || {
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut line = String::new();
                if reader.read_line(&mut line).is_err() {
                    return;
                }
                let mut parts = line.split_whitespace();
                let method = parts.next().unwrap_or("GET").to_string();
                let path = parts.next().unwrap_or("/").to_string();
                let mut headers = Vec::new();
                loop {
                    let mut h = String::new();
                    if reader.read_line(&mut h).unwrap_or(0) <= 2 {
                        break;
                    }
                    if let Some((n, v)) = h.trim_end().split_once(':') {
                        headers.push((n.trim().to_string(), v.trim().to_string()));
                    }
                }
                let len = headers
                    .iter()
                    .find(|(n, _)| n.eq_ignore_ascii_case("content-length"))
                    .and_then(|(_, v)| v.parse::<usize>().ok())
                    .unwrap_or(0);
                let mut body = vec![0; len];
                let _ = reader.read_exact(&mut body);
                let req = Req {
                    method,
                    path,
                    headers,
                    body: String::from_utf8_lossy(&body).into(),
                };
                let (status, hs, body) = reply(&req);
                let mut out = stream;
                let mut head = format!(
                    "HTTP/1.1 {status} X\r\nContent-Length: {}\r\nConnection: close\r\n",
                    body.len()
                );
                for (n, v) in hs {
                    head.push_str(&format!("{n}: {v}\r\n"));
                }
                head.push_str("\r\n");
                let _ = out.write_all(head.as_bytes());
                let _ = out.write_all(&body);
            });
        }
    });
    format!("http://{addr}")
}

fn agent_site() -> String {
    let origin = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
    let o = origin.clone();
    let base = serve_full(move |r| {
        let origin = o.lock().unwrap().clone();
        let json = |v: serde_json::Value| {
            (
                200,
                vec![("Content-Type", "application/json".to_string())],
                v.to_string().into_bytes(),
            )
        };
        let html = |b: &str| {
            (
                200,
                vec![
                    ("Content-Type", "text/html".to_string()),
                    ("Vary", "Accept".to_string()),
                ],
                b.as_bytes().to_vec(),
            )
        };
        match (r.method.as_str(), r.path.as_str()) {
            ("GET", "/") if r.header("accept").contains("text/markdown") => {
                (200, vec![("Content-Type", "text/markdown".to_string()), ("Vary", "Accept".to_string())], b"---\ntitle: Agent site\n---\n# Agent site\n".to_vec())
            }
            ("GET", "/") => html(&format!(
                r#"<!doctype html><html lang="en"><head><title>Agent ready site here</title>
<link rel="alternate" type="text/markdown" href="/index.md">
<script>navigator.modelContext && navigator.modelContext.registerTool({{name:"search"}});</script>
</head><body><header></header><nav><a href="/about">About</a><a href="/contact">Contact</a><a href="/privacy">Privacy</a><a href="/docs">Docs</a></nav>
<main><h1>Agent site</h1><p>{}</p><label for="q">Search</label><input id="q"><button>Go</button></main><footer></footer></body></html>"#,
                "Plenty of text for agents to read without running scripts. ".repeat(20)
            )),
            ("GET", "/robots.txt") => (200, vec![("Content-Type", "text/plain".to_string())], b"User-agent: GPTBot\nDisallow: /\n\nUser-agent: *\nAllow: /\n".to_vec()),
            ("GET", "/index.md") => (200, vec![("Content-Type", "text/markdown".to_string())], b"# Agent site\n".to_vec()),
            ("GET", "/llms.txt") => (200, vec![("Content-Type", "text/plain".to_string())], format!("# Agent site\n\n> For agents.\n\n## When to use\nUse this when you need X. Install the CLI with npx agent-cli.\n- [About]({origin}/about)\n").into_bytes()),
            ("GET", "/.well-known/ard.json") => json(serde_json::json!({"entries": [{"id": "urn:x", "name": "MCP", "type": "application/mcp", "url": format!("{origin}/mcp")}]})),
            ("GET", "/.well-known/mcp.json") => json(serde_json::json!({"url": format!("{origin}/mcp")})),
            ("GET", "/openapi.json") => json(serde_json::json!({"openapi": "3.1.0", "paths": {"/v1/items": {"get": {"operationId": "listItems", "summary": "List", "parameters": [{"name": "cursor", "in": "query", "schema": {"type": "string"}}], "responses": {"200": {"content": {"application/json": {}}}}}}}})),
            ("GET", "/about" | "/contact" | "/privacy" | "/docs") => html("<h1>Page</h1>"),
            ("POST", "/mcp") => {
                let v: serde_json::Value = serde_json::from_str(&r.body).unwrap_or_default();
                let id = v["id"].clone();
                let result = match v["method"].as_str().unwrap_or("") {
                    "initialize" => serde_json::json!({"protocolVersion": "2025-06-18", "serverInfo": {"name": "agent-site", "version": "1.0.0"}, "instructions": "Search the site.", "capabilities": {}}),
                    "tools/list" => serde_json::json!({"tools": [{"name": "search_items", "description": "Search the site's items by a free-text query and return matches.", "inputSchema": {"type": "object", "properties": {"q": {"type": "string"}}}, "annotations": {"readOnlyHint": true}}]}),
                    "resources/list" => serde_json::json!({"resources": [{"uri": "doc://a"}]}),
                    "notifications/initialized" => return (202, vec![], vec![]),
                    _ => return json(serde_json::json!({"jsonrpc": "2.0", "id": id, "error": {"code": -32601, "message": "Method not found"}})),
                };
                // Streamable HTTP may answer as an event stream.
                let body = format!("event: message\ndata: {}\n\n", serde_json::json!({"jsonrpc": "2.0", "id": id, "result": result}));
                (200, vec![("Content-Type", "text/event-stream".to_string()), ("Mcp-Session-Id", "s1".to_string())], body.into_bytes())
            }
            _ => (404, vec![("Content-Type", "text/plain".to_string())], b"Not found. See /llms.txt".to_vec()),
        }
    });
    *origin.lock().unwrap() = base.clone();
    base
}

#[test]
fn agent_readiness_on_a_local_site() {
    let base = agent_site();
    let rt = tokio::runtime::Runtime::new().unwrap();
    let report = rt.block_on(scan(&format!("{base}/"), ScanOptions::default()));
    let status = |id: &str| report.check(id).unwrap_or_else(|| panic!("{id}")).status;
    for id in [
        "ard-catalog",
        "ard-entries-valid",
        "robots-ai-policy-quality",
        "content-no-js",
        "mcp-well-known-discovery",
        "mcp-server",
        "mcp-transport-modern",
        "mcp-server-identity",
        "mcp-tool-listing",
        "mcp-tool-descriptions",
        "mcp-param-schemas",
        "mcp-tool-naming",
        "mcp-tool-annotations",
        "mcp-error-handling",
        "mcp-resource-listing",
        "markdown-negotiation",
        "markdown-negotiation-vary",
        "markdown-url-fallback",
        "markdown-link-alternate",
        "markdown-frontmatter",
        "agent-instruction",
        "cli-tool",
        "trust-anchors",
        "webmcp",
        "openapi-spec",
        "public-api",
        "pagination-shape",
        "ax-document-structure",
        "ax-form-labeling",
        "ax-tree-injection-safe",
    ] {
        assert_eq!(status(id), Status::Pass, "{id}: {:?}", report.check(id));
    }
    // GPTBot is a training crawler: blocking it is fine; the user agents aren't blocked.
    assert_eq!(status("robots-agent-user-policy"), Status::Pass);
    assert_eq!(status("dns-aid"), Status::Na);
    assert_eq!(status("x402-support"), Status::Na);
    assert_eq!(status("agent-friendly-404"), Status::Pass);
    assert_eq!(status("brand-search-accuracy"), Status::Skipped);
    assert!(report.agent_score > 40, "{}", report.agent_score);
    assert_eq!(report.layers.len(), 4);
    assert!(report.essentials > 0);
}

#[test]
fn scan_type_leaves_checks_out() {
    let base = agent_site();
    let rt = tokio::runtime::Runtime::new().unwrap();
    let content = rt.block_on(scan(
        &format!("{base}/"),
        ScanOptions {
            scan_type: ScanType::Content,
            ..Default::default()
        },
    ));
    assert_eq!(
        content.check("openapi-spec").unwrap().status,
        Status::Skipped
    );
    let api = rt.block_on(scan(
        &format!("{base}/"),
        ScanOptions {
            scan_type: ScanType::Api,
            ..Default::default()
        },
    ));
    assert_eq!(api.check("sitemap").unwrap().status, Status::Skipped);
    assert_eq!(api.check("openapi-spec").unwrap().status, Status::Pass);
}

#[test]
fn external_scan_never_sends_local_addresses() {
    assert!(super::external::refused("http://localhost:3000/").is_some());
    assert!(super::external::refused("https://kyzn.life/").is_none());
    let base = agent_site();
    let rt = tokio::runtime::Runtime::new().unwrap();
    let report = rt.block_on(scan(
        &format!("{base}/"),
        ScanOptions {
            external: true,
            ..Default::default()
        },
    ));
    let x = report.external.unwrap();
    assert!(x.score.is_none());
    assert!(x.missing.unwrap().contains("local"));
}

#[test]
fn external_results_fill_only_outside_checks() {
    let v = serde_json::json!({"score": 49, "grade": "C", "layers": [
        {"id": "discovery", "score": 6, "maxScore": 11, "checks": [
            {"id": "brand-search-accuracy", "status": "pass", "score": 3, "maxScore": 3, "details": "ranks first"},
            {"id": "sitemap", "status": "fail", "score": 0, "maxScore": 2}
        ]}
    ]});
    let (score, checks) = super::external::parse(&v);
    assert_eq!(score.score, Some(49));
    assert_eq!(score.layers[0].percent, 55);
    assert_eq!(checks.len(), 1);
    assert!(checks[0].external);
    assert_eq!(checks[0].status, Status::Pass);
}

#[test]
fn every_check_id_is_unique_and_scored_sensibly() {
    let mut ids = std::collections::HashSet::new();
    for c in super::CHECKS {
        assert!(ids.insert(c.id), "{} twice", c.id);
        assert!(c.max > 0, "{}", c.id);
        assert!(!c.fix.is_empty(), "{}", c.id);
    }
    assert!(super::CHECKS.len() >= 120);
}

#[test]
fn avif_size_from_ispe() {
    let mut b = vec![0, 0, 0, 24];
    b.extend_from_slice(b"ftypavif\0\0\0\0avifmif1");
    b.extend_from_slice(&[0, 0, 0, 20]);
    b.extend_from_slice(b"ispe");
    b.extend_from_slice(&[0, 0, 0, 0]);
    b.extend_from_slice(&1200u32.to_be_bytes());
    b.extend_from_slice(&630u32.to_be_bytes());
    assert_eq!(super::site::avif_size(&b), Some((1200, 630)));
    assert_eq!(super::image_size(&b).unwrap().2, "image/avif");
    assert!(super::site::avif_size(b"\x89PNG....").is_none());
}
