//! Opening pages: what the address field loads, the address it shows, a page's new windows, and
//! its links to other apps, asked about one at a time.

use gpui_kit::*;
use sawit_core::browser::{normalize_address, search_url};

use super::{BrowserView, engine};

impl BrowserView {
    pub(super) fn current_url(&self) -> String {
        self.engine
            .as_ref()
            .and_then(|e| e.url())
            .filter(|u| !u.is_empty())
            .unwrap_or_else(|| self.url.clone())
    }

    /// The tab is opened again: a port clicked in Ports (`url`) opens there, the Browser
    /// button (none) keeps the page.
    pub fn show(&mut self, url: Option<&str>, cx: &mut Context<Self>) {
        if let Some(url) = url {
            self.navigate(url, cx);
        }
    }

    /// Opens what the user typed (or an agent asked for). The keyboard stays where it is: an
    /// agent's load, or the dev server found at start, never takes it from the user.
    pub(super) fn navigate(&mut self, typed: &str, cx: &mut Context<Self>) {
        // Opened by the user or an agent: the page's links to other apps may ask again.
        self.external.reset();
        self.load(address_url(typed), cx);
    }

    /// Loads `url` in the tab, as it is.
    pub(super) fn load(&mut self, url: String, cx: &mut Context<Self>) {
        self.navigated = true;
        self.loading = true;
        self.url = url.clone();
        if let Some(engine) = &self.engine
            && let Err(e) = engine.load_url(&url)
        {
            eprintln!("sawit: {e}");
        }
        cx.notify();
    }

    /// Puts the page's address in the field, unless the user is typing there.
    pub(super) fn show_address(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.address.read(cx).focus_handle(cx).is_focused(window) {
            return;
        }
        let shown = if self.url == "about:blank" {
            ""
        } else {
            self.url.as_str()
        };
        if self.address.read(cx).value().as_ref() != shown {
            let shown = shown.to_string();
            self.address
                .update(cx, |a, cx| a.set_value(shown, window, cx));
        }
    }

    /// The page went to a link another app handles: asks the user before opening it, one
    /// link at a time; after a "Cancel" the page asks no more until the user or an agent opens
    /// a page or reloads it, which the question says.
    pub(super) fn open_external(
        &mut self,
        url: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.external.ask() {
            return;
        }
        let shown = match url.char_indices().nth(EXTERNAL_SHOWN) {
            Some((i, _)) => format!("{}…", &url[..i]),
            None => url.clone(),
        };
        let answer = window.prompt(
            PromptLevel::Info,
            "Open this link in another app?",
            Some(&format!(
                "The page asked to open {shown}. If you cancel, the page's links to other apps do \
                 nothing until you reload it or open another page."
            )),
            &["Open", "Cancel"],
            cx,
        );
        cx.spawn(async move |this, cx| {
            let open = matches!(answer.await, Ok(0));
            let _ = this.update(cx, |this, cx| {
                this.external.answered(open);
                if open {
                    // NSWorkspace: no process to wait for.
                    cx.open_url(&url);
                }
            });
        })
        .detach();
    }
}

/// What the address field (and an agent's navigate) loads for `typed`: its address when
/// WebKit can read it, else a search for the text (`file://my host/x` isn't a URL).
pub(super) fn address_url(typed: &str) -> String {
    let url = normalize_address(typed);
    if engine::loadable(&url) {
        url
    } else {
        search_url(typed)
    }
}

/// What a page's request for a new window (`window.open`, a `target=_blank` link) does.
#[derive(Debug, PartialEq)]
pub(super) enum NewWindow {
    Ignore,
    /// Opens in the tab.
    Load(String),
    /// A link another app handles: asked about.
    External(String),
}

/// What to do with the URL WebKit gives for a new window, from the page at `current`. It is
/// a URL already, so it isn't read as typed text: web pages load as they are; an empty or
/// `about:` window (`window.open()`, then `w.location = …`) is left alone, since the tab has
/// only one page; `data:` and `javascript:` are refused, as a page's own navigation to them
/// is; a local file only from a local page, a `blob:` only from its own origin; any other
/// scheme belongs to another app.
pub(super) fn new_window_action(url: &str, current: &str) -> NewWindow {
    let url = url.trim();
    let Some((scheme, rest)) = url.split_once(':').filter(|(s, _)| {
        s.starts_with(|c: char| c.is_ascii_alphabetic())
            && s.chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
    }) else {
        return NewWindow::Ignore;
    };
    let ok = match scheme.to_ascii_lowercase().as_str() {
        "http" | "https" => true,
        "blob" => origin(rest).is_some_and(|o| Some(o) == origin(current)),
        "file" => current.to_ascii_lowercase().starts_with("file:"),
        "about" | "data" | "javascript" => false,
        _ => return NewWindow::External(url.to_string()),
    };
    if ok && engine::loadable(url) {
        NewWindow::Load(url.to_string())
    } else {
        NewWindow::Ignore
    }
}

/// An http(s) URL's `scheme://host:port`, lowercased.
fn origin(url: &str) -> Option<String> {
    let (scheme, rest) = url.split_once("://")?;
    let scheme = scheme.to_ascii_lowercase();
    if scheme != "http" && scheme != "https" {
        return None;
    }
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    Some(format!("{scheme}://{}", authority.to_ascii_lowercase()))
}

/// How much of a link to another app the prompt shows.
const EXTERNAL_SHOWN: usize = 200;

/// Asks about a page's links to other apps one at a time, and stops asking once the user
/// says no, so a page can neither launch apps unasked nor pile up prompts.
#[derive(Default)]
pub(super) struct ExternalGate {
    asking: bool,
    refused: bool,
}

impl ExternalGate {
    /// Whether to ask about a link now; while asking, or after a "Cancel", it is dropped.
    pub(super) fn ask(&mut self) -> bool {
        if self.asking || self.refused {
            return false;
        }
        self.asking = true;
        true
    }

    pub(super) fn answered(&mut self, open: bool) {
        self.asking = false;
        self.refused = !open;
    }

    /// The user or an agent opened a page, or reloaded it: its links may ask again.
    pub(super) fn reset(&mut self) {
        self.refused = false;
    }
}
