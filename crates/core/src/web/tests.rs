//! The web module's tests: head parsing, favicon choice and decoding, local addresses. A loopback
//! server stands in for the web where a fetch is tested.

use super::favicon::{self, candidates, data_url, decode, largest_size, parse_reported};
use super::{head, is_local, origin};

const PAGE: &str = r#"<!doctype html><html lang="en"><head>
<title> Hello  world </title>
<meta name="description" content="A page">
<meta property="og:title" content="OG">
<link rel="icon" href="/favicon-16.png" sizes="16x16">
<link rel="icon" href="/favicon-32.png" sizes="32x32">
<link rel="apple-touch-icon" href="/apple.png">
<link rel="canonical" href="https://example.com/">
<script type="application/ld+json">{"@type":"Organization"}</script>
</head><body>
<h1>Title</h1><h2>Sub <span>part</span></h2>
<img src="a.png" alt="A"><img src="b.png">
<a href="/about" rel="nofollow">About us</a>
<label for="q">Search</label><input id="q"><input type="text"><button>Go</button>
<div onclick="x()">fake</div>
<script>var hidden = 1;</script>
</body></html>"#;

#[test]
fn head_reads_tags_headings_and_controls() {
    let h = head::parse(PAGE, "https://example.com/blog/");
    assert_eq!(h.lang.as_deref(), Some("en"));
    assert_eq!(h.title.as_deref(), Some("Hello world"));
    assert_eq!(h.meta("description"), Some("A page"));
    assert_eq!(h.meta("og:title"), Some("OG"));
    assert_eq!(h.canonical(), Some("https://example.com/"));
    assert_eq!(h.links[0].href, "https://example.com/favicon-16.png");
    assert_eq!(h.json_ld.len(), 1);
    assert_eq!(h.headings.len(), 2);
    assert_eq!(h.headings[1].text, "Sub part");
    assert_eq!(h.images[1].alt, None);
    assert_eq!(h.images[0].src, "https://example.com/blog/a.png");
    assert_eq!(h.anchors[0].href, "https://example.com/about");
    assert_eq!(h.anchors[0].rel, vec!["nofollow"]);
    let named: Vec<bool> = h.controls.iter().map(|c| c.named).collect();
    assert_eq!(named, vec![true, false, true]);
    assert_eq!(h.fake_buttons, 1);
    assert!(!h.text.contains("hidden"));
    assert!(h.text.contains("About us"));
}

#[test]
fn base_href_moves_relative_links() {
    let h = head::parse(
        r#"<head><base href="https://cdn.example.com/x/"><link rel="icon" href="i.png"></head>"#,
        "https://example.com/",
    );
    assert_eq!(h.links[0].href, "https://cdn.example.com/x/i.png");
}

#[test]
fn sizes_read_largest() {
    assert_eq!(largest_size("16x16 32x32"), Some(32));
    assert_eq!(largest_size("any"), Some(u32::MAX));
    assert_eq!(largest_size("bogus"), None);
}

#[test]
fn candidates_prefer_32px_then_touch_then_ico() {
    let h = head::parse(PAGE, "https://example.com/blog/");
    let c = candidates(&h.links, "https://example.com/blog/");
    assert_eq!(
        c,
        vec![
            "https://example.com/favicon-32.png",
            "https://example.com/favicon-16.png",
            "https://example.com/apple.png",
            "https://example.com/favicon.ico",
        ]
    );
}

#[test]
fn candidates_rank_svg_first_and_fall_back_to_ico() {
    let h = head::parse(
        r#"<link rel="icon" href="/i.png"><link rel="icon" type="image/svg+xml" href="/i.svg">"#,
        "http://localhost:3000/a",
    );
    let c = candidates(&h.links, "http://localhost:3000/a");
    assert_eq!(c[0], "http://localhost:3000/i.svg");
    assert_eq!(c.last().unwrap(), "http://localhost:3000/favicon.ico");
    assert_eq!(
        candidates(&[], "https://x.dev/p"),
        vec!["https://x.dev/favicon.ico"]
    );
}

#[test]
fn reported_links_parse_quoted_or_not() {
    let raw = r#"[{"rel":"Shortcut Icon","href":"https://a.b/i.ico","sizes":null,"type":"image/x-icon"}]"#;
    let quoted = serde_json::to_string(raw).unwrap();
    for text in [raw.to_string(), quoted] {
        let links = parse_reported(&text).unwrap();
        assert_eq!(links[0].rel, vec!["shortcut", "icon"]);
        assert!(favicon::is_icon(&links[0]));
    }
    assert!(parse_reported("null").is_none());
}

#[test]
fn data_urls_decode_base64_and_percent() {
    let (mime, bytes) = data_url("data:image/svg+xml,%3Csvg%3E").unwrap();
    assert_eq!(mime, "image/svg+xml");
    assert_eq!(bytes, b"<svg>");
    let (_, bytes) = data_url("data:image/png;base64,aGk=").unwrap();
    assert_eq!(bytes, b"hi");
}

fn png(w: u32, h: u32) -> Vec<u8> {
    let img = image::RgbaImage::from_pixel(w, h, image::Rgba([255, 0, 0, 255]));
    let mut out = Vec::new();
    img.write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png)
        .unwrap();
    out
}

#[test]
fn decode_scales_big_pictures_down() {
    let (w, h, rgba) = decode(&png(180, 90), "image/png", 64).unwrap();
    assert_eq!((w, h), (64, 32));
    assert_eq!(rgba.len(), 64 * 32 * 4);
    let (w, h, _) = decode(&png(16, 16), "", 64).unwrap();
    assert_eq!((w, h), (16, 16));
    assert!(decode(b"<html>nope</html>", "text/html", 64).is_none());
}

#[test]
fn decode_reads_ico_and_svg() {
    let mut ico = Vec::new();
    image::RgbaImage::from_pixel(32, 32, image::Rgba([0, 0, 255, 255]))
        .write_to(&mut std::io::Cursor::new(&mut ico), image::ImageFormat::Ico)
        .unwrap();
    assert_eq!(decode(&ico, "image/x-icon", 64).unwrap().0, 32);
    let svg = br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 20"><rect width="10" height="20" fill="#0f0"/></svg>"##;
    let (w, h, rgba) = decode(svg, "", 64).unwrap();
    assert_eq!((w, h), (32, 64));
    assert_eq!(&rgba[..4], &[0, 255, 0, 255]);
}

#[test]
fn local_addresses() {
    for url in [
        "http://localhost:3000/",
        "http://127.0.0.1/",
        "http://192.168.1.4:8080/",
        "http://my-mac.local/",
        "http://[::1]:5173/",
        "http://app.test/",
        "http://devbox/",
    ] {
        assert!(is_local(url), "{url}");
    }
    for url in ["https://example.com/", "https://8.8.8.8/"] {
        assert!(!is_local(url), "{url}");
    }
    assert_eq!(
        origin("http://localhost:3000/a?b").as_deref(),
        Some("http://localhost:3000")
    );
    assert_eq!(
        origin("https://x.dev:443/").as_deref(),
        Some("https://x.dev")
    );
}

/// A server on loopback that answers each request with `reply(path)`.
pub(crate) fn serve(
    reply: impl Fn(&str) -> (u16, Vec<(String, String)>, Vec<u8>) + Send + Sync + 'static,
) -> String {
    use std::io::{BufRead, BufReader, Write};
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
                let path = line.split_whitespace().nth(1).unwrap_or("/").to_string();
                let mut header = String::new();
                while reader.read_line(&mut header).is_ok_and(|n| n > 2) {
                    header.clear();
                }
                let (status, headers, body) = reply(&path);
                let mut out = stream;
                let mut head = format!(
                    "HTTP/1.1 {status} X\r\nContent-Length: {}\r\nConnection: close\r\n",
                    body.len()
                );
                for (n, v) in headers {
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

#[test]
fn favicon_found_from_html_then_cached() {
    let icon = png(48, 48);
    let base = serve(move |path| {
        match path {
        "/" => (
            200,
            vec![("Content-Type".into(), "text/html".into())],
            br#"<link rel="icon" href="/missing.png"><link rel="icon" href="/real.png" sizes="48x48">"#.to_vec(),
        ),
        "/real.png" => (200, vec![("Content-Type".into(), "image/png".into())], icon.clone()),
        _ => (404, vec![], b"no".to_vec()),
    }
    });
    let rt = tokio::runtime::Runtime::new().unwrap();
    let page = format!("{base}/");
    let got = rt.block_on(favicon::favicon(&page, None)).unwrap();
    assert_eq!(got.url, format!("{base}/real.png"));
    assert_eq!((got.width, got.height), (48, 48));
    assert!(favicon::cached(&format!("{base}/other")).is_some());
}

#[test]
fn fetch_records_redirects_and_errors() {
    let base = serve(|path| match path {
        "/a" => (301, vec![("Location".into(), "/b".into())], vec![]),
        "/b" => (200, vec![("X-Y".into(), "z".into())], b"done".to_vec()),
        _ => (404, vec![], vec![]),
    });
    let rt = tokio::runtime::Runtime::new().unwrap();
    let got = rt.block_on(super::fetch(&format!("{base}/a")));
    assert_eq!(got.status, 200);
    assert_eq!(got.redirects.len(), 1);
    assert_eq!(got.url, format!("{base}/b"));
    assert_eq!(got.header("x-y"), Some("z"));
    assert_eq!(got.text(), "done");
    let bad = rt.block_on(super::fetch("http://127.0.0.1:1/"));
    assert_eq!(bad.status, 0);
    assert!(bad.error.is_some());
    let bad = rt.block_on(super::fetch("ftp://x/"));
    assert!(bad.error.unwrap().contains("ftp"));
}
