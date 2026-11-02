//! The route crawl: the links read from a page, the limits that bound it, and a real crawl of a
//! dev server on loopback.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::time::Duration;

use crate::browser::routes::{Limits, crawl_with, discover, host_port, links_in, same_origin_path};

const ORIGIN: &str = "localhost:3000";

fn limits(pages: usize, links: usize) -> Limits {
    Limits {
        pages,
        links,
        request: Duration::from_secs(1),
        total: Duration::from_secs(30),
    }
}

/// Runs `crawl_with` on a site held in a map of paths to pages, answering `None` for the rest.
fn crawl_site(site: &HashMap<String, String>, limits: &Limits) -> crate::browser::routes::Crawled {
    let start = vec!["/".to_string()];
    futures::executor::block_on(crawl_with(&start, ORIGIN, limits, |path| {
        let page = site.get(&path).cloned();
        async move { page }
    }))
}

#[test]
fn the_links_of_a_page_are_its_servers_pages() {
    let html = r##"<html><body>
      <a href="/about">About</a> <A HREF='/blog/hello?ref=nav#top'>Post</A>
      <a class="x" href=changelog>Changelog</a>
      <a href="../up">Up</a>
      <a href="http://localhost:3000/pricing/">Pricing</a>
      <a href="http://127.0.0.1:3000/team">Team</a>
      <a href="https://example.com/away">Away</a> <a href="//cdn.example.com/x">CDN</a>
      <a href="http://localhost:4000/other">Other server</a>
      <a href="mailto:hi@example.com">Mail</a> <a href="#section">Here</a>
      <a href="/logo.png">Logo</a> <a href="/_next/static/x">Chunk</a> <a href="/api/users">API</a>
      <abbr href="/not-a-link">x</abbr> <a href="/about">About again</a>
      <link href="/style.css" rel="stylesheet">
    </body></html>"##;
    let links = links_in(html, "/docs/start", ORIGIN);
    assert_eq!(
        links,
        [
            "/about",
            "/blog/hello",
            "/docs/changelog",
            "/up",
            "/pricing",
            "/team"
        ]
    );
}

#[test]
fn relative_links_resolve_against_their_page() {
    let path = |href: &str, page: &str| same_origin_path(href, page, ORIGIN);
    assert_eq!(path("./a/../b", "/x/y").as_deref(), Some("/x/b"));
    assert_eq!(path("b", "/").as_deref(), Some("/b"));
    assert_eq!(path("/", "/x").as_deref(), Some("/"));
    assert_eq!(path("/../../etc", "/").as_deref(), None);
    assert_eq!(path("javascript:alert(1)", "/"), None);
}

/// A site that links on for ever is crawled only as far as the page limit.
#[test]
fn the_crawl_stops_at_its_page_limit() {
    let mut site = HashMap::new();
    for n in 0..500 {
        let at = if n == 0 {
            "/".to_string()
        } else {
            format!("/p/{n}")
        };
        site.insert(at, format!("<a href=\"/p/{}\">next</a>", n + 1));
    }
    let crawled = crawl_site(&site, &limits(12, 1000));
    assert_eq!(crawled.fetched, 12);
    assert_eq!(crawled.pages.len(), 12);
    assert!(crawled.capped);
    assert_eq!(crawled.pages[11], "/p/11");
}

/// A page with more links than the crawl may find is cut at the link limit.
#[test]
fn the_crawl_stops_at_its_link_limit() {
    let mut site = HashMap::new();
    let many: String = (0..300)
        .map(|n| format!("<a href=\"/item/{n}\">{n}</a>"))
        .collect();
    site.insert("/".to_string(), many);
    let crawled = crawl_site(&site, &limits(50, 40));
    // The start counts among the pages found.
    assert_eq!(crawled.links.len(), 39);
    assert!(crawled.capped);
    assert!(crawled.fetched <= 50);
}

#[test]
fn a_small_site_is_crawled_whole() {
    let mut site = HashMap::new();
    site.insert(
        "/".to_string(),
        "<a href=/a>a</a><a href=/b>b</a>".to_string(),
    );
    site.insert(
        "/a".to_string(),
        "<a href=/>home</a><a href=/b>b</a>".to_string(),
    );
    site.insert("/b".to_string(), "<a href=/missing>gone</a>".to_string());
    let crawled = crawl_site(&site, &limits(30, 200));
    assert_eq!(crawled.pages, ["/", "/a", "/b"]);
    assert_eq!(crawled.links, ["/a", "/b", "/missing"]);
    assert_eq!(crawled.fetched, 4);
    assert!(!crawled.capped);
}

#[test]
fn addresses_name_their_host_and_port() {
    let hp = |url: &str| host_port(url).map(|(h, p)| format!("{h} {p}"));
    assert_eq!(
        hp("http://localhost:5173/x").as_deref(),
        Some("localhost 5173")
    );
    assert_eq!(hp("http://[::1]:8080").as_deref(), Some("[::1] 8080"));
    assert_eq!(hp("http://127.0.0.1").as_deref(), Some("127.0.0.1 80"));
    assert_eq!(hp("https://localhost").as_deref(), Some("localhost 443"));
    assert_eq!(hp("ftp://localhost"), None);
}

/// A dev server whose app Sawit can't read is crawled from `/` over loopback: its pages are the
/// routes, and files it links to aren't.
#[tokio::test]
async fn an_app_without_file_routes_is_crawled() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for stream in listener.incoming().take(8) {
            let Ok(mut stream) = stream else { continue };
            let mut request = [0u8; 2048];
            let n = stream.read(&mut request).unwrap_or(0);
            let line = String::from_utf8_lossy(&request[..n]).to_string();
            let path = line.split_whitespace().nth(1).unwrap_or("/").to_string();
            let body = match path.as_str() {
                "/" => "<a href=\"/pricing\">Pricing</a><a href=\"/logo.svg\">x</a>",
                "/pricing" => "<a href=\"/\">Home</a><a href=\"/contact\">Contact</a>",
                "/contact" => "<p>Write to us</p>",
                _ => "",
            };
            let (status, kind) = match body.is_empty() {
                true => ("404 Not Found", "text/plain"),
                false => ("200 OK", "text/html; charset=utf-8"),
            };
            let answer = format!(
                "HTTP/1.1 {status}\r\nContent-Type: {kind}\r\nContent-Length: {}\r\n\r\n{body}",
                body.len()
            );
            let _ = stream.write_all(answer.as_bytes());
        }
    });
    let app = tempfile::tempdir().unwrap();
    std::fs::write(
        app.path().join("package.json"),
        r#"{"dependencies":{"vite":"7"}}"#,
    )
    .unwrap();
    let base = format!("http://127.0.0.1:{port}");
    let found = discover(app.path(), &base, &limits(10, 50)).await;
    assert_eq!(found.framework, None);
    let paths: Vec<&str> = found.routes.iter().map(|r| r.path.as_str()).collect();
    assert_eq!(paths, ["/", "/contact", "/pricing"]);
    assert!(found.crawl_error.is_none());
}
