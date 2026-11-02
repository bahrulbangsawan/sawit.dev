//! A fake dev server on loopback: the probe and its cache of answers, which servers are asked,
//! other hosts and paths that leave the API refused, and calls with their bodies.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

use super::http::position;
use super::*;
use crate::ports::PortJob;

/// A cut-down OpenAPI document, as Wrangler 4.118's Local Explorer serves at its API's root.
const SPEC: &str = r#"{"openapi":"3.0.3","info":{"title":"Local Explorer API","version":"0.0.1"},"servers":[{"url":"/cdn-cgi/local/explorer/api"}],"paths":{"/storage/kv/namespaces":{"get":{}},"/d1/database/{database_id}/raw":{"post":{}},"/local/workers":{"get":{}}}}"#;
const WORKERS: &str = r#"{"success":true,"result":[{"isSelf":true,"name":"shop-api","bindings":{"kv":[{"id":"a","bindingName":"CACHE"}],"d1":[{"id":"1","bindingName":"DB"}],"r2":[]}}]}"#;

/// What the fake server got: each request's head and body.
type Seen = Arc<Mutex<Vec<String>>>;

/// A dev server on 127.0.0.1:0 that answers with `answer(request)`: the status, the content
/// type and the body, sent chunked when `chunked`. It counts and keeps what it is sent.
async fn serve(
    answer: fn(&str) -> (u16, &'static str, String),
    chunked: bool,
) -> (SocketAddr, Arc<AtomicUsize>, Seen) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let hits = Arc::new(AtomicUsize::new(0));
    let seen: Seen = Arc::default();
    let (h, s) = (hits.clone(), seen.clone());
    tokio::spawn(async move {
        loop {
            let Ok((mut socket, _)) = listener.accept().await else {
                return;
            };
            h.fetch_add(1, Ordering::SeqCst);
            let s = s.clone();
            tokio::spawn(async move {
                let mut buf = Vec::new();
                let mut chunk = [0u8; 4096];
                // The head, then as much body as Content-Length says.
                loop {
                    let n = socket.read(&mut chunk).await.unwrap_or(0);
                    if n == 0 {
                        break;
                    }
                    buf.extend_from_slice(&chunk[..n]);
                    if let Some(end) = position(&buf, b"\r\n\r\n") {
                        let head = String::from_utf8_lossy(&buf[..end]).to_lowercase();
                        let len = head
                            .lines()
                            .find_map(|l| l.strip_prefix("content-length:"))
                            .and_then(|v| v.trim().parse::<usize>().ok())
                            .unwrap_or(0);
                        if buf.len() >= end + 4 + len {
                            break;
                        }
                    }
                }
                let request = String::from_utf8_lossy(&buf).into_owned();
                s.lock().unwrap().push(request.clone());
                let (status, kind, body) = answer(&request);
                let reply = if chunked {
                    let (a, b) = body.split_at(body.len() / 2);
                    format!(
                        "HTTP/1.1 {status} X\r\nContent-Type: {kind}\r\nTransfer-Encoding: chunked\r\n\r\n{:x}\r\n{a}\r\n{:x}\r\n{b}\r\n0\r\n\r\n",
                        a.len(),
                        b.len()
                    )
                } else {
                    format!(
                        "HTTP/1.1 {status} X\r\nContent-Type: {kind}\r\nContent-Length: {}\r\n\r\n{body}",
                        body.len()
                    )
                };
                let _ = socket.write_all(reply.as_bytes()).await;
            });
        }
    });
    (addr, hits, seen)
}

fn explorer_server(request: &str) -> (u16, &'static str, String) {
    let line = request.lines().next().unwrap_or_default();
    match line.split_whitespace().nth(1).unwrap_or_default() {
        API_PATH => (200, "application/json", SPEC.into()),
        p if p == format!("{API_PATH}/local/workers") => (200, "application/json", WORKERS.into()),
        p if p.starts_with(API_PATH) => (
            200,
            "application/json",
            format!(r#"{{"method":"{}"}}"#, line.split(' ').next().unwrap()),
        ),
        _ => (404, "text/html", "<h1>Not found</h1>".into()),
    }
}

fn app_server(_: &str) -> (u16, &'static str, String) {
    (
        200,
        "text/html",
        "<!doctype html><title>My app</title>".into(),
    )
}

fn job(root: u32, ports: &[u16]) -> PortJob {
    PortJob {
        root,
        pids: vec![root],
        label: "pnpm dev".into(),
        tool: "pnpm".into(),
        cmdline: "pnpm dev".into(),
        listeners: vec![(root + 1, "workerd serve --binary".into())],
        ports: ports.iter().map(|p| (*p, BindAddr::LoopbackV4)).collect(),
        cwd: Some("/Users/me/shop/apps/api".into()),
        dev: true,
    }
}

/// The probe finds a fake dev server's explorer from its OpenAPI document, chunked or not, and
/// finds none on a server that answers with its own HTML or nothing at all.
#[tokio::test]
async fn the_probe_finds_an_explorer_by_its_openapi_document() {
    for chunked in [false, true] {
        let (addr, _, seen) = serve(explorer_server, chunked).await;
        let spec = probe(addr).await.unwrap().expect("an explorer");
        assert_eq!(spec.title.as_deref(), Some("Local Explorer API"));
        assert!(spec.has("GET /local/workers"));
        assert!(spec.has("POST /d1/database/{database_id}/raw"));
        let request = seen.lock().unwrap()[0].clone();
        assert!(request.starts_with("GET /cdn-cgi/local/explorer/api HTTP/1.1\r\n"));
        assert!(request.contains(&format!("Host: localhost:{}", addr.port())));
    }
    let (app, _, _) = serve(app_server, false).await;
    assert_eq!(probe(app).await.unwrap(), None);
    // A port nothing listens on any more.
    let closed = {
        let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
        l.local_addr().unwrap()
    };
    assert_eq!(probe(closed).await.unwrap(), None);
    // Other JSON isn't an explorer.
    assert_eq!(parse_spec(br#"{"openapi":"3.0.0"}"#), None);
    assert_eq!(parse_spec(br#"{"swagger":"2.0","paths":{}}"#), None);
}

/// Each dev server is asked once while the same process holds its port: a second look reads
/// the cache, and a new process on the port is asked again. A server serving it on several
/// ports is one explorer, on its lowest; a plain app and a database are no explorer.
#[tokio::test]
async fn explorers_are_found_once_per_port_and_process() {
    let (api, hits, _) = serve(explorer_server, false).await;
    let (internal, _, _) = serve(explorer_server, false).await;
    let (app, app_hits, _) = serve(app_server, false).await;
    let cache = Mutex::new(Cache::default());
    let mut db = job(30, &[5432]);
    db.dev = false;
    let mut jobs = vec![
        job(10, &[api.port(), internal.port()]),
        job(20, &[app.port()]),
        db,
    ];
    jobs[0].ports.sort_by_key(|(port, _)| *port);
    let lowest = jobs[0].ports[0].0;

    let found = find_with(&cache, &jobs).await;
    assert_eq!(found.len(), 1);
    assert_eq!((found[0].port, found[0].pid), (lowest, 10));
    assert_eq!(
        found[0].url(),
        format!("http://localhost:{lowest}/cdn-cgi/local/explorer")
    );
    assert!(found[0].runs_in(Path::new("/Users/me/shop")));
    assert!(!found[0].runs_in(Path::new("/Users/me/blog")));
    let (first, app_first) = (hits.load(Ordering::SeqCst), app_hits.load(Ordering::SeqCst));
    assert_eq!(app_first, 1);

    let again = find_with(&cache, &jobs).await;
    assert_eq!(again, found);
    assert_eq!(hits.load(Ordering::SeqCst), first, "asked again");
    assert_eq!(
        app_hits.load(Ordering::SeqCst),
        1,
        "the miss was asked again at once"
    );

    jobs[0].root = 11;
    let restarted = find_with(&cache, &jobs).await;
    assert_eq!(restarted[0].pid, 11);
    assert!(
        hits.load(Ordering::SeqCst) > first,
        "a new process wasn't asked"
    );
}

/// A miss is asked again after a few seconds, a find after a minute.
#[test]
fn answers_are_kept_for_a_while() {
    let mut cache = Cache::default();
    let t = Instant::now();
    let spec = parse_spec(SPEC.as_bytes()).unwrap();
    cache.record(8787, 1, Some(spec.clone()), t);
    cache.record(5173, 2, None, t);
    assert_eq!(
        cache.known(8787, 1, t + Duration::from_secs(30)),
        Some(Some(&spec))
    );
    assert_eq!(cache.known(8787, 1, t + Duration::from_secs(61)), None);
    assert_eq!(cache.known(8787, 9, t), None);
    assert_eq!(cache.known(5173, 2, t + Duration::from_secs(1)), Some(None));
    assert_eq!(cache.known(5173, 2, t + Duration::from_secs(9)), None);
    // Each miss in a row keeps the next one longer, up to a few minutes.
    cache.record(5173, 2, None, t);
    assert_eq!(cache.known(5173, 2, t + Duration::from_secs(9)), Some(None));
    assert_eq!(cache.known(5173, 2, t + Duration::from_secs(17)), None);
    for _ in 0..10 {
        cache.record(5173, 2, None, t);
    }
    assert_eq!(
        cache.known(5173, 2, t + Duration::from_secs(200)),
        Some(None)
    );
    assert_eq!(cache.known(5173, 2, t + Duration::from_secs(241)), None);
    // A find starts the count again.
    cache.record(5173, 2, Some(spec.clone()), t);
    cache.record(5173, 2, None, t);
    assert_eq!(cache.known(5173, 2, t + Duration::from_secs(9)), None);
    cache.retain(|port| port == 8787);
    assert_eq!(cache.known(5173, 2, t), None);
}

/// Only dev servers that may be Workers ones are asked: a Django server's log doesn't fill
/// with 404s every few seconds.
#[tokio::test]
async fn only_workers_dev_servers_are_asked() {
    let (django, hits, _) = serve(app_server, false).await;
    let mut other = job(50, &[django.port()]);
    other.cmdline = "python manage.py runserver".into();
    other.label = other.cmdline.clone();
    other.listeners = vec![(50, "python manage.py runserver".into())];
    other.cwd = None;
    assert!(!may_serve(&other));
    assert!(
        find_with(&Mutex::default(), &[other.clone()])
            .await
            .is_empty()
    );
    assert_eq!(hits.load(Ordering::SeqCst), 0);
    for line in [
        "pnpm exec wrangler dev",
        "node /app/node_modules/.bin/vite",
        "cf dev",
    ] {
        other.cmdline = line.into();
        assert!(may_serve(&other), "{line}");
    }
    // A folder with a Worker config, whatever started the server.
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("wrangler.toml"), "name = \"w\"\n").unwrap();
    other.cmdline = "npm start".into();
    other.cwd = Some(dir.path().display().to_string());
    assert!(may_serve(&other));
}

/// Nothing but loopback is ever asked or called: a server bound to a LAN address alone isn't
/// probed, and a call to another host is refused before anything is sent.
#[tokio::test]
async fn addresses_other_than_loopback_are_refused() {
    assert_eq!(
        loopback(BindAddr::AnyV4),
        Some(IpAddr::V4(Ipv4Addr::LOCALHOST))
    );
    assert_eq!(
        loopback(BindAddr::LoopbackV6),
        Some(IpAddr::V6(Ipv6Addr::LOCALHOST))
    );
    assert_eq!(
        loopback(BindAddr::Specific("192.168.1.5".parse().unwrap())),
        None
    );
    assert_eq!(
        loopback(BindAddr::Specific("127.0.0.2".parse().unwrap())),
        Some("127.0.0.2".parse().unwrap())
    );
    for remote in [
        "192.168.1.5:8787",
        "10.0.0.1:80",
        "[2001:db8::1]:8787",
        "1.1.1.1:443",
    ] {
        let addr: SocketAddr = remote.parse().unwrap();
        let probed = probe(addr).await;
        assert!(probed.unwrap_err().contains("isn't a loopback address"));
        let called = request(addr, Method::Get, "d1/database", None).await;
        assert!(called.unwrap_err().contains("isn't a loopback address"));
    }
    // A LAN-only dev server is skipped without a probe.
    let mut lan = job(40, &[8787]);
    lan.ports = vec![(8787, BindAddr::Specific("192.168.1.5".parse().unwrap()))];
    assert!(find_with(&Mutex::default(), &[lan]).await.is_empty());
}

/// Paths stay under the API: absolute URLs, `..` and `.` segments, encoded or not, and
/// backslashes are refused; the API's own prefix and a query are kept.
#[test]
fn paths_that_leave_the_api_are_refused() {
    let ok = |p: &str| api_path(p).unwrap();
    assert_eq!(
        ok("storage/kv/namespaces"),
        format!("{API_PATH}/storage/kv/namespaces")
    );
    assert_eq!(ok("/d1/database"), format!("{API_PATH}/d1/database"));
    assert_eq!(ok(""), API_PATH);
    assert_eq!(ok("/"), API_PATH);
    assert_eq!(
        ok(&format!("{API_PATH}/r2/buckets")),
        format!("{API_PATH}/r2/buckets")
    );
    assert_eq!(
        ok("local/scheduled?worker=shop-api&cron=*/5+*+*+*+*"),
        format!("{API_PATH}/local/scheduled?worker=shop-api&cron=*/5+*+*+*+*")
    );
    assert_eq!(
        ok("storage/kv/namespaces/a/values/user%3A1"),
        format!("{API_PATH}/storage/kv/namespaces/a/values/user%3A1")
    );
    // `/etc/passwd` is a path under the API too: it can't leave it.
    assert_eq!(ok("/etc/passwd"), format!("{API_PATH}/etc/passwd"));
    for bad in [
        "../../../",
        "local/../../secret",
        "storage/kv/..",
        "./d1",
        "%2e%2e/%2e%2e/",
        "%2E%2E%2Fadmin",
        "%252e%252e/x",
        "/cdn-cgi/local/explorer/../x",
        "..\\..\\x",
        "http://evil.example/x",
        "https://127.0.0.1:8787/cdn-cgi/local/explorer/api",
        "//evil.example/x",
        "d1/database\r\nX-Injected: 1",
        "d1 database",
        "d1#frag",
    ] {
        assert!(api_path(bad).is_err(), "{bad} was let through");
    }
}

/// Only GET goes without confirmation; every method that can write needs it.
#[test]
fn writes_need_confirmation() {
    assert!(check_confirmed(Method::Get, false).is_ok());
    for method in ["POST", "put", "Patch", "DELETE"] {
        let method = Method::parse(method).unwrap();
        let refused = check_confirmed(method, false).unwrap_err();
        assert!(refused.contains("confirm: true"), "{refused}");
        assert!(check_confirmed(method, true).is_ok());
    }
    assert_eq!(Method::parse("").unwrap(), Method::Get);
    assert!(Method::parse("TRACE").is_err());
}

/// A call goes to the API path with its method and body, and the Workers the dev server runs
/// are read from `GET /local/workers`.
#[tokio::test]
async fn calls_reach_the_api_with_their_body() {
    let (addr, _, seen) = serve(explorer_server, true).await;
    let sql = br#"{"sql":"select 1"}"#;
    let r = request(
        addr,
        Method::Post,
        "d1/database/1/raw",
        Some((sql, "application/json")),
    )
    .await
    .unwrap();
    assert_eq!(r.status, 200);
    assert_eq!(r.body, br#"{"method":"POST"}"#);
    assert!(!r.truncated);
    let sent = seen.lock().unwrap()[0].clone();
    assert!(sent.starts_with("POST /cdn-cgi/local/explorer/api/d1/database/1/raw HTTP/1.1"));
    assert!(sent.contains("Content-Type: application/json"));
    assert!(sent.ends_with(r#"{"sql":"select 1"}"#));

    let workers = local_workers(addr).await;
    assert_eq!(
        workers,
        [LocalWorker {
            name: "shop-api".into(),
            is_self: true,
            bindings: vec![("kv".into(), "CACHE".into()), ("d1".into(), "DB".into())],
        }]
    );
}
