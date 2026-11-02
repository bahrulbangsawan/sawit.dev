//! What the app's URL scheme ([`super::SCHEME`]) answers WebKit for a screen's page: the files of
//! that screen's own folder, and nothing else. A request for another screen's files, for a path
//! that leaves the folder (`..`, a link that points out of it), or with a method other than GET is
//! refused, and every answer carries a Content-Security-Policy ([`CSP`]) that keeps the page off
//! the network: what it loads comes from its folder, or from `data:` and `blob:` URLs.

use std::io::ErrorKind;
use std::path::Path;

use super::{INDEX, SCHEME, mime, read};

/// The policy every answer carries: the page loads only from its own screen (`'self'`, its
/// scheme and name) and from `data:` and `blob:` URLs, runs its own scripts and styles, inline
/// ones too, and sends no form anywhere. So nothing reaches the network, a page's fonts, images
/// and `fetch` included.
pub const CSP: &str = "default-src 'self' data: blob:; script-src 'self' 'unsafe-inline'; \
                       style-src 'self' 'unsafe-inline'; img-src 'self' data: blob:; \
                       font-src 'self' data:; media-src 'self' data: blob:; connect-src 'self'; \
                       frame-src 'self'; form-action 'none'; base-uri 'self'";

/// An answer to one request.
#[derive(Clone, Debug, PartialEq)]
pub struct Served {
    pub status: u16,
    pub mime: &'static str,
    pub body: Vec<u8>,
}

impl Served {
    fn refused(status: u16, why: &str) -> Served {
        Served {
            status,
            mime: "text/plain; charset=utf-8",
            body: why.as_bytes().to_vec(),
        }
    }
}

/// Answers `method` `url` for screen `name`, whose folder is `folder`. The page is
/// `sawit-screen://<name>/index.html`; a path that ends in `/` is that folder's `index.html`.
pub fn serve(folder: &Path, name: &str, method: &str, url: &str) -> Served {
    if !matches!(method, "GET" | "HEAD") {
        return Served::refused(405, "A screen's files are only read.");
    }
    let Some(rest) = url.strip_prefix(SCHEME).and_then(|r| r.strip_prefix("://")) else {
        return Served::refused(400, "Not a screen's URL.");
    };
    let (host, path) = rest.split_once('/').unwrap_or((rest, ""));
    if !host.eq_ignore_ascii_case(name) {
        return Served::refused(403, "That is another screen's file.");
    }
    let path = path.split(['?', '#']).next().unwrap_or_default();
    let rel = match super::segments(INDEX, &format!("/{path}")) {
        Some(parts) if parts.is_empty() => INDEX.to_string(),
        Some(parts) if path.ends_with('/') => format!("{}/{INDEX}", parts.join("/")),
        Some(parts) => parts.join("/"),
        None => return Served::refused(403, "That is outside the screen's folder."),
    };
    let rel = match folder.join(&rel).is_dir() {
        true => format!("{rel}/{INDEX}"),
        false => rel,
    };
    match read(folder, &rel) {
        Ok(body) => Served {
            status: 200,
            mime: mime(&rel),
            body: if method == "HEAD" { Vec::new() } else { body },
        },
        Err(e) if e.kind() == ErrorKind::NotFound => Served::refused(404, "No such file."),
        Err(e) if e.kind() == ErrorKind::PermissionDenied => {
            Served::refused(403, "That is outside the screen's folder.")
        }
        Err(e) => Served::refused(403, &format!("The file can't be served: {e}.")),
    }
}

/// Whether a page of screen `name` may go to `url`: only to another page of its own folder
/// (`about:blank` too, which WebKit loads first). Links elsewhere are refused, so a screen never
/// leaves its folder for the network or another app.
pub fn may_navigate(name: &str, url: &str) -> bool {
    if url == "about:blank" {
        return true;
    }
    let Some(rest) = url.strip_prefix(SCHEME).and_then(|r| r.strip_prefix("://")) else {
        return false;
    };
    let host = rest.split(['/', '?', '#']).next().unwrap_or_default();
    host.eq_ignore_ascii_case(name)
}
