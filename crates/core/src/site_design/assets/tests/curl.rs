//! curl's command lines, and what it reports after a body or a download.

use super::*;
use crate::site_design::assets::curl::{curl_args_with, download_report, parse_fetched};

#[test]
fn curl_args_carry_the_referer() {
    let a = curl_args(
        "https://x.com/a.png",
        Path::new("/tmp/a.png"),
        "https://x.com/",
        52_428_800,
    );
    let s: Vec<String> = a.iter().map(|x| x.to_string_lossy().into_owned()).collect();
    let at = |f: &str| s.iter().position(|x| x == f).unwrap();
    assert_eq!(s[at("-e") + 1], "https://x.com/");
    assert_eq!(s[at("--max-filesize") + 1], "52428800");
    assert_eq!(s.last().unwrap(), "https://x.com/a.png");
    assert_eq!(s[s.len() - 2], "--");
    assert_eq!(s[at("--proto") + 1], "=http,https");
    assert_eq!(s[at("--proto-redir") + 1], "=http,https");
}

/// curl reads no `~/.curlrc` (`-q`, which must come first): a curlrc's `header`,
/// `cookie` or `user` lines would go to whatever host a page lists.
#[test]
fn curl_reads_no_curlrc() {
    let download = curl_args(
        "https://x.com/a.png",
        Path::new("/tmp/a.png"),
        "https://x.com/",
        1,
    );
    assert_eq!(download[0], "-q");
    assert_eq!(
        curl_args_with("https://x.com/a.css", None, "https://x.com/", "10", "1")[0],
        "-q"
    );
}

#[test]
fn curl_says_where_a_download_came_from() {
    let url = "https://cdn.example/a.png";
    assert_eq!(
        download_report("https://cdn.example/a.png\n93.184.215.14\n0"),
        (url, "93.184.215.14", false)
    );
    assert_eq!(
        download_report("https://cdn.example/a.png\n10.0.0.5\n1"),
        (url, "10.0.0.5", true)
    );
    // A curl before 8.7 (macOS 13 and 14) writes nothing for `proxy_used`.
    assert_eq!(
        download_report("https://cdn.example/a.png\n93.184.215.14\n"),
        (url, "93.184.215.14", false)
    );
    assert_eq!(download_report(""), ("", "", false));
}

#[test]
fn curl_reports_follow_the_body() {
    let f = parse_fetched("a{}\nb{}\n\ntext/css\nhttps://c.dev/x.css\n1.2.3.4").unwrap();
    assert_eq!(f.text, "a{}\nb{}\n");
    assert_eq!(f.content_type, "text/css");
    assert_eq!(f.final_url, "https://c.dev/x.css");
    assert_eq!(f.remote_ip, "1.2.3.4");
    assert_eq!(parse_fetched("no report"), None);
}
