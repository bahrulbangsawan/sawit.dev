//! Downloads end to end: what is saved, skipped or refused, and the size caps.

use super::*;
use crate::site_design::assets::download::{BLOB_MSG, download_for, is_file_url};

/// A Save into a folder an earlier Save filled names a new file after the ones there, so
/// it never writes over a file of another asset that had the same name.
#[tokio::test]
async fn saves_dont_write_over_earlier_files() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("images")).unwrap();
    std::fs::write(dir.path().join("images/logo.png"), b"earlier").unwrap();
    let png = format!("data:image/png;base64,{}", enc(&[7u8; 10]));
    let items = vec![asset(AssetKind::Image, Some(&png), None, "logo")];
    let page = "https://x.com/";
    let out = download_all(items, dir.path(), page, page, false, 1 << 20, |_, _| {}).await;
    assert_eq!(out[0].file.as_deref(), Some("images/logo-2.png"));
    assert_eq!(
        std::fs::read(dir.path().join("images/logo.png")).unwrap(),
        b"earlier"
    );
}

#[test]
fn file_pages_by_their_url() {
    assert!(is_file_url("file:///Users/a/index.html"));
    assert!(is_file_url(" FILE:///x"));
    assert!(!is_file_url("https://a.dev/file://x"));
}

#[tokio::test]
async fn file_urls_only_from_file_pages() {
    let src = tempfile::tempdir().unwrap();
    std::fs::write(src.path().join("a.png"), [1u8; 10]).unwrap();
    let url = format!("file://{}/a.png", src.path().display());
    let mk = || vec![asset(AssetKind::Image, Some(&url), None, "a")];

    let dir = tempfile::tempdir().unwrap();
    let out = download_all(
        mk(),
        dir.path(),
        "https://x.com/",
        "https://x.com/",
        false,
        1 << 20,
        |_, _| {},
    )
    .await;
    assert!(out[0].error.is_some() && out[0].file.is_none());
    assert!(!dir.path().join("images").exists());
    let hosts = vec![asset(
        AssetKind::Image,
        Some("file:///etc/hosts"),
        None,
        "h",
    )];
    let out = download_all(
        hosts,
        dir.path(),
        "https://x.com/",
        "https://x.com/",
        false,
        1 << 20,
        |_, _| {},
    )
    .await;
    assert!(out[0].error.is_some());
    assert!(!dir.path().join("images/h.png").exists());
    // A page saying it is a file doesn't make it one.
    let out = download_all(
        mk(),
        dir.path(),
        "file:///tmp/index.html",
        "https://x.com/",
        false,
        1 << 20,
        |_, _| {},
    )
    .await;
    assert!(out[0].error.is_some());
    assert!(!dir.path().join("images").exists());

    let out = download_all(
        mk(),
        dir.path(),
        "file:///tmp/index.html",
        "file:///tmp/index.html",
        false,
        1 << 20,
        |_, _| {},
    )
    .await;
    assert_eq!(out[0].file.as_deref(), Some("images/a.png"));
    assert_eq!(out[0].bytes, 10);
    assert!(dir.path().join("images/a.png").exists());

    let js = vec![asset(
        AssetKind::Image,
        Some("javascript:alert(1)"),
        None,
        "j",
    )];
    let out = download_all(
        js,
        dir.path(),
        "https://x.com/",
        "https://x.com/",
        false,
        1 << 20,
        |_, _| {},
    )
    .await;
    assert!(out[0].error.is_some());
}

#[tokio::test]
async fn failed_downloads_are_recorded() {
    let png = format!("data:image/png;base64,{}", enc(&[7u8; 600]));
    let items = vec![
        asset(AssetKind::Svg, None, Some("<svg/>"), "mark"),
        asset(AssetKind::Image, Some(&png), None, "one"),
        asset(AssetKind::Image, Some(&png), None, "two"),
        asset(AssetKind::Video, Some("https://x.com/v.mp4"), None, "clip"),
        asset(AssetKind::Image, Some("blob:https://x.com/abc"), None, "b"),
    ];
    let dir = tempfile::tempdir().unwrap();
    let out = download_all(
        items,
        dir.path(),
        "https://x.com/",
        "https://x.com/",
        false,
        500,
        |_, _| {},
    )
    .await;
    assert_eq!(out.len(), 5);
    assert_eq!(out[0].file.as_deref(), Some("svg/mark.svg"));
    assert_eq!(out[0].bytes, 6);
    assert_eq!(out[1].file.as_deref(), Some("images/one.png"));
    assert_eq!(out[1].bytes, 600);
    assert!(dir.path().join("images/one.png").exists());
    assert!(out[2].error.as_deref().unwrap().starts_with("Skipped: the"));
    assert!(out[2].file.is_none());
    assert_eq!(out[3].error.as_deref(), Some(VIDEO_SKIPPED));
    assert!(is_skipped_video(&out[3]));
    assert!(!out.iter().take(3).any(is_skipped_video));
    assert_eq!(out[4].error.as_deref(), Some(BLOB_MSG));
}

/// The size caps hold for the bytes written: a gzip answer (64 MB of zeros arrive as
/// 64 KB) was inflated by curl past `--max-filesize`, which counts the bytes on the wire,
/// and downloads running at once each had the whole budget left to them.
#[tokio::test]
async fn downloads_and_sheets_stay_under_their_caps() {
    let origin = zeros_server();
    let page = format!("{origin}/");
    let budget = 1 << 20;
    let gz = vec![asset(
        AssetKind::Image,
        Some(&format!("{origin}/gz/{}/a.png", 64 << 20)),
        None,
        "gz",
    )];
    let dir = tempfile::tempdir().unwrap();
    download_all(gz, dir.path(), &page, &page, false, budget, |_, _| {}).await;
    let written = bytes_in(dir.path());
    assert!(written <= budget, "a gzip answer wrote {written} bytes");

    let plain = |name: &str, n: u64| {
        asset(
            AssetKind::Image,
            Some(&format!("{origin}/plain/{n}/{name}.png")),
            None,
            name,
        )
    };
    let dir = tempfile::tempdir().unwrap();
    let items = vec![plain("big", 3 << 20)];
    download_all(items, dir.path(), &page, &page, false, budget, |_, _| {}).await;
    let written = bytes_in(dir.path());
    assert!(written <= budget, "one download wrote {written} bytes");

    let dir = tempfile::tempdir().unwrap();
    let items = vec![plain("one", 800 << 10), plain("two", 800 << 10)];
    download_all(items, dir.path(), &page, &page, false, budget, |_, _| {}).await;
    let written = bytes_in(dir.path());
    assert!(
        written <= budget,
        "two downloads at once wrote {written} bytes"
    );

    let sheet = fetch_text(&format!("{origin}/gz/{}/a.css", 64 << 20), &page).await;
    if let Ok(sheet) = sheet {
        assert!(
            sheet.text.len() <= 5 << 20,
            "a gzip sheet came back as {} bytes",
            sheet.text.len()
        );
    }
}

/// A page on the web can't have Sawit fetch, and save, what is on this Mac or its
/// network; a page there gets its files. Where the page is follows the tab's own URL.
#[tokio::test]
async fn a_web_page_cant_save_what_is_on_this_mac() {
    let origin = zeros_server();
    let item = || {
        vec![asset(
            AssetKind::Image,
            Some(&format!("{origin}/plain/10/a.png")),
            None,
            "a",
        )]
    };
    let dir = tempfile::tempdir().unwrap();
    // The page may say it is local; the tab's own URL says it isn't.
    let own = format!("{origin}/");
    let out = download_all(
        item(),
        dir.path(),
        &own,
        "https://shop.example/",
        false,
        1 << 20,
        |_, _| {},
    )
    .await;
    assert!(out[0].error.is_some(), "{:?}", out[0]);
    assert_eq!(bytes_in(dir.path()), 0);
    let out = download_all(item(), dir.path(), &own, &own, false, 1 << 20, |_, _| {}).await;
    assert_eq!(out[0].file.as_deref(), Some("images/a.png"), "{:?}", out[0]);
}

/// A page on `localhost` saves its assets from another local name: `127.0.0.1` (the
/// page's own server here) and `localhost`.
#[tokio::test]
async fn a_localhost_page_saves_its_assets_from_any_local_name() {
    let origin = zeros_server();
    let port = origin.rsplit(':').next().unwrap().to_string();
    let page = format!("http://localhost:{port}/");
    let items = vec![
        asset(
            AssetKind::Image,
            Some(&format!("http://127.0.0.1:{port}/plain/10/hero.png")),
            None,
            "hero",
        ),
        asset(
            AssetKind::Image,
            Some(&format!("http://localhost:{port}/plain/20/logo.png")),
            None,
            "logo",
        ),
    ];
    let dir = tempfile::tempdir().unwrap();
    let out = download_all(items, dir.path(), &page, &page, false, MAX_TOTAL, |_, _| {}).await;
    assert_eq!(
        out[0].file.as_deref(),
        Some("images/hero.png"),
        "{:?}",
        out[0]
    );
    assert_eq!(
        out[1].file.as_deref(),
        Some("images/logo.png"),
        "{:?}",
        out[1]
    );
    assert_eq!(bytes_in(dir.path()), 30);
}

/// A `.test` page (resolved to this Mac) saves what Vite serves it from `127.0.0.1`; had
/// its name resolved to the web, that file would be left out.
#[tokio::test]
async fn a_test_page_saves_what_vite_serves_it() {
    let origin = zeros_server();
    let page = "http://myapp.test/";
    let vite = || {
        vec![asset(
            AssetKind::Image,
            Some(&format!("{origin}/plain/10/app.png")),
            None,
            "app",
        )]
    };
    let dir = tempfile::tempdir().unwrap();
    let out = download_for(
        vite(),
        dir.path(),
        page,
        page,
        Some(true),
        false,
        1 << 20,
        |_, _| {},
    )
    .await;
    assert_eq!(
        out[0].file.as_deref(),
        Some("images/app.png"),
        "{:?}",
        out[0]
    );
    let dir = tempfile::tempdir().unwrap();
    let out = download_for(
        vite(),
        dir.path(),
        page,
        page,
        Some(false),
        false,
        1 << 20,
        |_, _| {},
    )
    .await;
    assert_eq!(
        out[0].error.as_deref(),
        Some("Left out: it is on this Mac or its network.")
    );
    assert_eq!(bytes_in(dir.path()), 0);
}
