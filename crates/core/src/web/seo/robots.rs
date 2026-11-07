//! `robots.txt` read in Rust: its groups of rules per user agent, sitemaps, Content Signals and
//! other directives, and whether a path is allowed to an agent (longest match wins, `Allow` on a
//! tie, `*` and `$` patterns, as RFC 9309 says).

/// One `User-agent` group.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Group {
    /// The user agents, lower case.
    pub agents: Vec<String>,
    /// `(allow, pattern)` rules, in order.
    pub rules: Vec<(bool, String)>,
}

/// A parsed `robots.txt`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Robots {
    pub groups: Vec<Group>,
    pub sitemaps: Vec<String>,
    /// `Content-Signal:` values (`search=yes, ai-train=no`), as written.
    pub content_signals: Vec<String>,
    /// Every other directive, `(name lower case, value)`, e.g. `schemamap`, `agentmap`.
    pub other: Vec<(String, String)>,
}

impl Robots {
    pub fn parse(text: &str) -> Robots {
        let mut out = Robots::default();
        let mut current: Option<Group> = None;
        // Agent lines right after each other share one group.
        let mut last_was_agent = false;
        for line in text.lines() {
            let line = line.split('#').next().unwrap_or("").trim();
            let Some((key, value)) = line.split_once(':') else {
                continue;
            };
            let key = key.trim().to_ascii_lowercase();
            let value = value.trim().to_string();
            match key.as_str() {
                "user-agent" => {
                    if !last_was_agent {
                        if let Some(g) = current.take() {
                            out.groups.push(g);
                        }
                        current = Some(Group::default());
                    }
                    if let Some(g) = current.as_mut() {
                        g.agents.push(value.to_ascii_lowercase());
                    }
                    last_was_agent = true;
                    continue;
                }
                "allow" | "disallow" => {
                    if let Some(g) = current.as_mut() {
                        // An empty `Disallow:` allows everything: no rule.
                        if !value.is_empty() {
                            g.rules.push((key == "allow", value));
                        }
                    }
                }
                "sitemap" => out.sitemaps.push(value),
                "content-signal" => out.content_signals.push(value),
                _ => out.other.push((key, value)),
            }
            last_was_agent = false;
        }
        if let Some(g) = current {
            out.groups.push(g);
        }
        out
    }

    /// The group that applies to `agent` (its product token, any case): the one naming it, else
    /// `*`.
    pub fn group_for(&self, agent: &str) -> Option<&Group> {
        let agent = agent.to_ascii_lowercase();
        self.groups
            .iter()
            .find(|g| g.agents.contains(&agent))
            .or_else(|| {
                self.groups
                    .iter()
                    .find(|g| g.agents.iter().any(|a| a == "*"))
            })
    }

    /// Whether `agent` may fetch `path`.
    pub fn allowed(&self, agent: &str, path: &str) -> bool {
        let Some(group) = self.group_for(agent) else {
            return true;
        };
        let mut best: Option<(usize, bool)> = None;
        for (allow, pattern) in &group.rules {
            if matches(pattern, path) {
                let len = pattern.len();
                best = match best {
                    Some((l, a)) if l > len || (l == len && a) => Some((l, a)),
                    _ => Some((len, *allow)),
                };
            }
        }
        best.is_none_or(|(_, allow)| allow)
    }

    /// Whether a group names `agent` itself (not through `*`).
    pub fn names(&self, agent: &str) -> bool {
        let agent = agent.to_ascii_lowercase();
        self.groups.iter().any(|g| g.agents.contains(&agent))
    }

    /// A directive other than the rules, by name (lower case).
    pub fn directive(&self, name: &str) -> Option<&str> {
        self.other
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.as_str())
    }
}

/// Whether a robots pattern (`/a*b$`) matches `path`.
pub fn matches(pattern: &str, path: &str) -> bool {
    let (pattern, anchored) = match pattern.strip_suffix('$') {
        Some(p) => (p, true),
        None => (pattern, false),
    };
    let parts: Vec<&str> = pattern.split('*').collect();
    let mut at = 0;
    for (i, part) in parts.iter().enumerate() {
        if i == 0 {
            if !path.starts_with(part) {
                return false;
            }
            at = part.len();
            continue;
        }
        match path[at..].find(part) {
            Some(found) => at += found + part.len(),
            None => return false,
        }
    }
    if anchored {
        // The last part must end the path.
        let last = parts.last().copied().unwrap_or("");
        return path.ends_with(last) && (parts.len() > 1 || at == path.len());
    }
    true
}

/// The AI agents and crawlers whose rules are read, as `(token, what it does)`.
pub const AI_TRAINING: &[&str] = &[
    "gptbot",
    "google-extended",
    "ccbot",
    "anthropic-ai",
    "claudebot",
    "applebot-extended",
    "bytespider",
    "meta-externalagent",
];
pub const AI_SEARCH: &[&str] = &["oai-searchbot", "perplexitybot", "claude-searchbot"];
pub const AI_USER: &[&str] = &["chatgpt-user", "claude-user", "perplexity-user"];
