//! A route as a framework's files lay it out: its segments, static or dynamic, the path it is
//! when it has no dynamic part, and whether a path the server links to is one of its pages.

/// One segment of a route's path.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Segment {
    /// A name the path holds as it is: `changelog`.
    Static(String),
    /// One segment of any value: `[slug]`, `$slug`, `:slug`.
    Param(String),
    /// One segment or none: `[[lang]]`, `{-$lang}`, `($lang)`.
    Optional(String),
    /// One segment or more: `[...path]`, `$`.
    Rest(String),
    /// Any number of segments, none too: `[[...path]]`.
    OptionalRest(String),
}

impl Segment {
    fn is_static(&self) -> bool {
        matches!(self, Segment::Static(_))
    }

    /// Whether the route stands without this segment: an optional one.
    fn may_be_left_out(&self) -> bool {
        matches!(self, Segment::Optional(_) | Segment::OptionalRest(_))
    }

    /// The segment as the canvas writes it, whatever the framework's syntax: `[slug]`,
    /// `[[lang]]`, `[...path]`, `[[...path]]`.
    fn written(&self) -> String {
        match self {
            Segment::Static(name) => name.clone(),
            Segment::Param(name) => format!("[{name}]"),
            Segment::Optional(name) => format!("[[{name}]]"),
            Segment::Rest(name) => format!("[...{name}]"),
            Segment::OptionalRest(name) => format!("[[...{name}]]"),
        }
    }
}

/// A route of a framework's files: its segments, and the file that makes it, relative to the
/// app's folder.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pattern {
    pub segments: Vec<Segment>,
    pub file: String,
}

impl Pattern {
    /// Whether every segment is a name as it is.
    pub fn is_static(&self) -> bool {
        self.segments.iter().all(Segment::is_static)
    }

    /// The path the route answers without a value for any of its segments: its static segments,
    /// when the rest may be left out (`/docs/[[...path]]` is `/docs`). `None` while one needs a
    /// value.
    pub fn bare_path(&self) -> Option<String> {
        let mut names = Vec::new();
        for segment in &self.segments {
            match segment {
                Segment::Static(name) => names.push(name.as_str()),
                other if other.may_be_left_out() => {}
                _ => return None,
            }
        }
        Some(format!("/{}", names.join("/")))
    }

    /// The route as the canvas writes it: `/`, `/blog/[slug]`.
    pub fn written(&self) -> String {
        let parts: Vec<String> = self.segments.iter().map(Segment::written).collect();
        format!("/{}", parts.join("/"))
    }

    /// Whether its pages are pages rather than files or an API ([`not_a_page`]): it isn't under
    /// `/api`, and its last segment, when that is a name, has no extension.
    pub fn is_page(&self) -> bool {
        let names: Vec<&str> = (self.segments.iter())
            .map(|s| match s {
                Segment::Static(name) => name.as_str(),
                _ => "x",
            })
            .collect();
        !not_a_page(&format!("/{}", names.join("/")))
    }

    /// Whether `path` (`/blog/hello`, no query) is a page of this route.
    pub fn matches(&self, path: &str) -> bool {
        let parts: Vec<&str> = path.split('/').filter(|p| !p.is_empty()).collect();
        matches_from(&self.segments, &parts)
    }
}

fn matches_from(segments: &[Segment], parts: &[&str]) -> bool {
    let Some((first, rest)) = segments.split_first() else {
        return parts.is_empty();
    };
    match first {
        Segment::Static(name) => parts.split_first().is_some_and(|(part, more)| {
            part.eq_ignore_ascii_case(name) && matches_from(rest, more)
        }),
        Segment::Param(_) => parts
            .split_first()
            .is_some_and(|(_, more)| matches_from(rest, more)),
        Segment::Optional(_) => {
            matches_from(rest, parts) || (!parts.is_empty() && matches_from(rest, &parts[1..]))
        }
        Segment::Rest(_) => (1..=parts.len()).any(|n| matches_from(rest, &parts[n..])),
        Segment::OptionalRest(_) => (0..=parts.len()).any(|n| matches_from(rest, &parts[n..])),
    }
}

/// Whether the route `path` (`/rss.xml`, `/api/users`) is a file or an API rather than a page:
/// its last segment has an extension other than `.html`, or it is under `/api`.
pub fn not_a_page(path: &str) -> bool {
    let first = path.split('/').find(|p| !p.is_empty());
    if first.is_some_and(|p| p.eq_ignore_ascii_case("api")) {
        return true;
    }
    let last = path.rsplit('/').next().unwrap_or_default();
    last.rsplit_once('.')
        .is_some_and(|(stem, ext)| !stem.is_empty() && !ext.eq_ignore_ascii_case("html"))
}
