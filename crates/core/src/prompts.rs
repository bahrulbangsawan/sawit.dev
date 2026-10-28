//! Prompt library: saved prompts with tags and favourites, search,
//! `{{variable}}` placeholders, and importing prompts kept in a WhatsApp
//! chat ("Export chat" → `_chat.txt`). Stored as one JSON file.

use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};

use crate::files::write_text_atomic;
use crate::{CoreError, Result};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Prompt {
    pub id: String,
    pub title: String,
    pub body: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub favorite: bool,
    /// Unix seconds.
    pub created_at: u64,
    pub updated_at: u64,
    #[serde(default)]
    pub used_count: u64,
    #[serde(default)]
    pub last_used_at: u64,
}

/// Lowercase, without `#`, trimmed, unique, in first-seen order.
pub fn normalize_tags<S: AsRef<str>>(tags: &[S]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for t in tags {
        let t = t
            .as_ref()
            .trim()
            .trim_start_matches('#')
            .trim()
            .to_lowercase();
        if !t.is_empty() && !out.contains(&t) {
            out.push(t);
        }
    }
    out
}

fn new_id() -> String {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or_default();
    format!("{nanos:x}-{:x}", NEXT.fetch_add(1, Ordering::Relaxed))
}

/// Seconds since the Unix epoch.
pub fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or_default()
}

impl Prompt {
    pub fn new<S: AsRef<str>>(title: &str, body: &str, tags: &[S]) -> Prompt {
        Prompt {
            id: new_id(),
            title: title.trim().to_string(),
            body: body.to_string(),
            tags: normalize_tags(tags),
            favorite: false,
            created_at: 0,
            updated_at: 0,
            used_count: 0,
            last_used_at: 0,
        }
    }

    fn matches(&self, words: &[String], tags: &[String]) -> bool {
        tags.iter().all(|t| self.tags.contains(t))
            && words.iter().all(|w| {
                self.title.to_lowercase().contains(w)
                    || self.body.to_lowercase().contains(w)
                    || self.tags.iter().any(|t| t.contains(w.as_str()))
            })
    }

    /// When it was last touched: used or edited.
    fn recency(&self) -> u64 {
        self.last_used_at.max(self.updated_at)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Library {
    #[serde(rename = "prompts")]
    pub items: Vec<Prompt>,
}

impl Library {
    /// A missing file is an empty library; a corrupt one is an error (so
    /// it is never overwritten with nothing).
    pub fn load(path: &Path) -> Result<Library> {
        match std::fs::read_to_string(path) {
            Ok(text) => serde_json::from_str(&text)
                .map_err(|e| CoreError::Parse(format!("{}: {e}", path.display()))),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Library::default()),
            Err(e) => Err(e.into()),
        }
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let text =
            serde_json::to_string_pretty(self).map_err(|e| CoreError::Parse(e.to_string()))?;
        write_text_atomic(path, &format!("{text}\n"))
    }

    /// Adds a prompt stamped `now`; returns its id.
    pub fn add(&mut self, mut prompt: Prompt, now: u64) -> String {
        prompt.created_at = now;
        prompt.updated_at = now;
        let id = prompt.id.clone();
        self.items.push(prompt);
        id
    }

    pub fn get(&self, id: &str) -> Option<&Prompt> {
        self.items.iter().find(|p| p.id == id)
    }

    /// Edits a prompt in place and stamps it; false if it doesn't exist.
    pub fn update(&mut self, id: &str, now: u64, edit: impl FnOnce(&mut Prompt)) -> bool {
        match self.items.iter_mut().find(|p| p.id == id) {
            Some(p) => {
                edit(p);
                p.tags = normalize_tags(&p.tags);
                p.updated_at = now;
                true
            }
            None => false,
        }
    }

    pub fn remove(&mut self, id: &str) -> bool {
        let before = self.items.len();
        self.items.retain(|p| p.id != id);
        self.items.len() != before
    }

    /// Records a copy/use (moves the prompt up the list).
    pub fn mark_used(&mut self, id: &str, now: u64) {
        if let Some(p) = self.items.iter_mut().find(|p| p.id == id) {
            p.used_count += 1;
            p.last_used_at = now;
        }
    }

    /// Prompts matching every word (title, body or tag) and every `#tag`;
    /// favourites first, then most recently used or edited.
    pub fn search(&self, query: &str) -> Vec<&Prompt> {
        let mut words = Vec::new();
        let mut tags = Vec::new();
        for part in query.split_whitespace() {
            match part.strip_prefix('#') {
                Some(t) if !t.is_empty() => tags.push(t.to_lowercase()),
                _ => words.push(part.to_lowercase()),
            }
        }
        let mut found: Vec<&Prompt> = self
            .items
            .iter()
            .filter(|p| p.matches(&words, &tags))
            .collect();
        found.sort_by(|a, b| {
            b.favorite
                .cmp(&a.favorite)
                .then(b.recency().cmp(&a.recency()))
                .then(a.title.cmp(&b.title))
        });
        found
    }

    /// Every tag with how many prompts use it, most used first.
    pub fn tags(&self) -> Vec<(String, usize)> {
        let mut counts: HashMap<&str, usize> = HashMap::new();
        for p in &self.items {
            for t in &p.tags {
                *counts.entry(t).or_default() += 1;
            }
        }
        let mut out: Vec<(String, usize)> = counts
            .into_iter()
            .map(|(t, n)| (t.to_string(), n))
            .collect();
        out.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        out
    }

    /// Adds prompts whose text isn't in the library yet; returns how many.
    pub fn import(&mut self, prompts: Vec<Prompt>) -> usize {
        let mut added = 0;
        for p in prompts {
            let body = p.body.trim();
            if self.items.iter().any(|q| q.body.trim() == body) {
                continue;
            }
            let at = p.created_at;
            self.add(p, at);
            added += 1;
        }
        added
    }
}

/// `{{name}}` placeholders, in order of first appearance.
pub fn variables(body: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut rest = body;
    while let Some(start) = rest.find("{{") {
        let after = &rest[start + 2..];
        let Some(end) = after.find("}}") else { break };
        let name = after[..end].trim();
        if !name.is_empty() && !name.contains('{') && !out.iter().any(|n| n == name) {
            out.push(name.to_string());
        }
        rest = &after[end + 2..];
    }
    out
}

/// Replaces `{{name}}` (spaces inside allowed) with its value; unknown
/// names are left as they are.
pub fn fill(body: &str, values: &HashMap<String, String>) -> String {
    let mut out = String::with_capacity(body.len());
    let mut rest = body;
    while let Some(start) = rest.find("{{") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        let Some(end) = after.find("}}") else {
            out.push_str(&rest[start..]);
            return out;
        };
        let name = after[..end].trim();
        match values.get(name) {
            Some(v) => out.push_str(v),
            None => out.push_str(&rest[start..start + 2 + end + 2]),
        }
        rest = &after[end + 2..];
    }
    out.push_str(rest);
    out
}

/// Messages shorter than this aren't prompts ("ok", "thanks").
const MIN_PROMPT_CHARS: usize = 20;

/// The message text after a WhatsApp export line header, if the line starts
/// a message: iPhone `[29/10/26 04.57.59] Name: text`, Android
/// `29/10/26 04.57 - Name: text` or `10/29/26, 5:01 PM - Name: text`.
fn message_start(line: &str) -> Option<&str> {
    let rest = if let Some(after) = line.strip_prefix('[') {
        let (stamp, rest) = after.split_once("] ")?;
        if !stamp.contains('/') {
            return None;
        }
        rest
    } else {
        let first = line.chars().next()?;
        if !first.is_ascii_digit() {
            return None;
        }
        let (stamp, rest) = line.split_once(" - ")?;
        if !stamp.contains('/') || stamp.len() > 24 {
            return None;
        }
        rest
    };
    // "Name: text"; lines without a sender are system notices.
    let (_, text) = rest.split_once(": ")?;
    Some(text)
}

fn is_noise(body: &str) -> bool {
    const MARKERS: [&str; 9] = [
        "omitted",
        "<Media omitted>",
        "<attached:",
        "end-to-end encrypted",
        "This message was deleted",
        "You deleted this message",
        "Missed voice call",
        "Missed video call",
        "<This message was edited>",
    ];
    body.chars().count() < MIN_PROMPT_CHARS
        || (body.lines().count() == 1 && MARKERS.iter().any(|m| body.contains(m)))
}

/// Title from the first line, at most 60 characters.
fn title_from(body: &str) -> String {
    let first = body
        .lines()
        .find(|l| !l.trim().is_empty())
        .unwrap_or("")
        .trim();
    if first.chars().count() <= 60 {
        return first.to_string();
    }
    let cut: String = first.chars().take(59).collect();
    format!("{}…", cut.trim_end())
}

/// Prompts from a WhatsApp chat export (one per message, multi-line
/// messages kept whole; media, system notices and short replies skipped),
/// tagged `whatsapp` and stamped `now`.
pub fn import_whatsapp(text: &str, now: u64) -> Vec<Prompt> {
    let clean = |s: &str| {
        s.replace(['\u{200e}', '\u{200f}', '\u{feff}'], "")
            .replace('\u{202f}', " ")
    };
    let mut messages: Vec<String> = Vec::new();
    for raw in text.lines() {
        let line = clean(raw);
        match message_start(&line) {
            Some(start) => messages.push(start.to_string()),
            None => {
                if let Some(last) = messages.last_mut() {
                    last.push('\n');
                    last.push_str(&line);
                }
            }
        }
    }
    messages
        .into_iter()
        .map(|m| m.trim().to_string())
        .filter(|m| !is_noise(m))
        .map(|body| {
            let mut p = Prompt::new(&title_from(&body), &body, &["whatsapp"]);
            p.created_at = now;
            p.updated_at = now;
            p
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn library() -> Library {
        let mut lib = Library::default();
        lib.add(
            Prompt::new(
                "Review SQL",
                "Review this query for {{dialect}}:\n{{sql}}",
                &["sql", "review"],
            ),
            100,
        );
        lib.add(
            Prompt::new(
                "Commit message",
                "Write a commit message for this diff",
                &["git"],
            ),
            200,
        );
        lib.add(
            Prompt::new(
                "Explain error",
                "Explain this Rust compiler error",
                &["rust"],
            ),
            300,
        );
        lib
    }

    #[test]
    fn new_prompts_get_ids_and_timestamps() {
        let lib = library();
        assert_eq!(lib.items.len(), 3);
        let ids: std::collections::HashSet<_> = lib.items.iter().map(|p| &p.id).collect();
        assert_eq!(ids.len(), 3, "ids are unique");
        assert_eq!(lib.items[0].created_at, 100);
        assert_eq!(lib.items[0].updated_at, 100);
        // Tags are normalised: lowercase, no '#', no duplicates.
        let p = Prompt::new("x", "y", &["#SQL", "sql", " Git "]);
        assert_eq!(p.tags, vec!["sql", "git"]);
    }

    #[test]
    fn search_matches_every_word_in_title_body_or_tags() {
        let lib = library();
        let titles =
            |q: &str| -> Vec<String> { lib.search(q).iter().map(|p| p.title.clone()).collect() };
        assert_eq!(titles("review"), vec!["Review SQL"]);
        assert_eq!(titles("COMMIT diff"), vec!["Commit message"]);
        assert_eq!(titles("#rust"), vec!["Explain error"]);
        assert_eq!(titles("#sql query"), vec!["Review SQL"]);
        assert!(titles("#sql commit").is_empty());
        assert_eq!(titles("").len(), 3);
    }

    #[test]
    fn favourites_then_most_recently_used_come_first() {
        let mut lib = library();
        let explain = lib.items[2].id.clone();
        let review = lib.items[0].id.clone();
        lib.mark_used(&explain, 500);
        assert_eq!(lib.search("")[0].title, "Explain error");
        lib.update(&review, 600, |p| p.favorite = true);
        let order: Vec<_> = lib.search("").iter().map(|p| p.title.clone()).collect();
        assert_eq!(order, vec!["Review SQL", "Explain error", "Commit message"]);
        assert_eq!(lib.get(&explain).unwrap().used_count, 1);
    }

    #[test]
    fn update_and_remove() {
        let mut lib = library();
        let id = lib.items[1].id.clone();
        assert!(lib.update(&id, 900, |p| p.title = "Commit msg".into()));
        let p = lib.get(&id).unwrap();
        assert_eq!(
            (p.title.as_str(), p.updated_at, p.created_at),
            ("Commit msg", 900, 200)
        );
        assert!(lib.remove(&id));
        assert!(lib.get(&id).is_none());
        assert!(!lib.remove(&id));
        assert!(!lib.update("missing", 1, |_| {}));
    }

    #[test]
    fn all_tags_with_counts() {
        let mut lib = library();
        lib.add(Prompt::new("Another", "x", &["sql"]), 400);
        assert_eq!(
            lib.tags(),
            vec![
                ("sql".to_string(), 2),
                ("git".to_string(), 1),
                ("review".to_string(), 1),
                ("rust".to_string(), 1)
            ]
        );
    }

    #[test]
    fn saves_and_loads_atomically() {
        let d = tempfile::tempdir().unwrap();
        let file = d.path().join("prompts.json");
        assert!(
            Library::load(&file).unwrap().items.is_empty(),
            "missing file = empty library"
        );
        let lib = library();
        lib.save(&file).unwrap();
        assert_eq!(Library::load(&file).unwrap(), lib);
        std::fs::write(&file, "{ not json").unwrap();
        assert!(
            Library::load(&file).is_err(),
            "corrupt file is an error, not silently emptied"
        );
    }

    #[test]
    fn variables_are_found_in_order_without_duplicates() {
        assert_eq!(
            variables("Hi {{name}}, review {{ sql }} for {{name}} in {{dialect}}"),
            vec!["name", "sql", "dialect"]
        );
        assert!(variables("no vars, {{}} or {{ }} or {single}").is_empty());
    }

    #[test]
    fn filling_variables() {
        let values = std::collections::HashMap::from([
            ("sql".to_string(), "SELECT 1".to_string()),
            ("dialect".to_string(), "PostgreSQL".to_string()),
        ]);
        assert_eq!(
            fill("Review for {{dialect}}:\n{{ sql }} ({{other}})", &values),
            "Review for PostgreSQL:\nSELECT 1 ({{other}})"
        );
    }

    #[test]
    fn imports_an_iphone_whatsapp_export() {
        let chat = "\
[29/10/26 04.57.59] Rul: \u{200e}Messages and calls are end-to-end encrypted.
[29/10/26 05.01.02] Rul: Kamu adalah senior engineer.
Review kode berikut dengan teliti.
Fokus ke bug.
[29/10/26 05.02.10] Rul: \u{200e}image omitted
[29/10/26 05.03.00] Rul: ok
[30/10/26 09.15.44] Rul: Buatkan commit message dari diff ini, singkat dan jelas, bahasa Inggris
";
        let prompts = import_whatsapp(chat, 1000);
        let bodies: Vec<_> = prompts.iter().map(|p| p.body.as_str()).collect();
        assert_eq!(
            bodies,
            vec![
                "Kamu adalah senior engineer.\nReview kode berikut dengan teliti.\nFokus ke bug.",
                "Buatkan commit message dari diff ini, singkat dan jelas, bahasa Inggris",
            ]
        );
        assert_eq!(prompts[0].title, "Kamu adalah senior engineer.");
        assert_eq!(prompts[0].tags, vec!["whatsapp"]);
        assert!(prompts[1].title.chars().count() <= 60);
    }

    #[test]
    fn imports_an_android_whatsapp_export() {
        let chat = "\
29/10/26 04.57 - Rul: Tolong jelaskan error ini secara detail
dan beri solusi
29/10/26 05.00 - Rul: <Media omitted>
10/29/26, 5:01 PM - Rul: Write tests first, then the implementation
";
        let prompts = import_whatsapp(chat, 1000);
        assert_eq!(prompts.len(), 2);
        assert_eq!(
            prompts[0].body,
            "Tolong jelaskan error ini secara detail\ndan beri solusi"
        );
        assert_eq!(
            prompts[1].body,
            "Write tests first, then the implementation"
        );
    }

    #[test]
    fn import_skips_prompts_already_in_the_library() {
        let mut lib = library();
        let chat = "[29/10/26 05.01.02] Rul: Write a commit message for this diff\n[29/10/26 05.02.02] Rul: Brand new prompt text here\n";
        let added = lib.import(import_whatsapp(chat, 1000));
        assert_eq!(added, 1);
        assert_eq!(lib.items.len(), 4);
        assert_eq!(
            lib.import(import_whatsapp(chat, 1000)),
            0,
            "re-import adds nothing"
        );
    }
}
