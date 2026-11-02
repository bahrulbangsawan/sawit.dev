//! The console messages and requests the page agent reports, as each browser tab keeps them.

use std::collections::VecDeque;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::redact::{redact_body, redact_url};

/// Console messages and requests kept per browser tab, newest last.
pub const MAX_CONSOLE: usize = 500;
pub const MAX_NETWORK: usize = 300;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConsoleEntry {
    /// `log`, `info`, `warning`, `error` or `debug`.
    pub level: String,
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line: Option<u32>,
    /// Milliseconds since the page started.
    #[serde(default)]
    pub time_ms: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NetworkEntry {
    pub method: String,
    pub url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<u16>,
    /// `fetch`, `xhr`, or the resource's initiator (`script`, `img`, …).
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_body: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub response_body: Option<String>,
    /// The request never got a response (blocked, offline, CORS).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(default)]
    pub time_ms: f64,
}

impl NetworkEntry {
    pub fn failed(&self) -> bool {
        self.error.is_some() || self.status.is_some_and(|s| s >= 400)
    }

    /// The entry as the network log keeps it, so that no password or token reaches an agent
    /// (`browser_network`), as `browser_snapshot` shows no password field's value: the values of
    /// fields named like secrets ([`secret_name`]) are [`REDACTED`] in the URL (its query, its
    /// fragment and a password before its host) and in the request and response bodies (JSON,
    /// also cut short, and forms). The rest is kept as it was, so the shape still shows. The page
    /// agent has already put [`REDACTED`] wherever a request carried what is typed in the page's
    /// password fields, whatever their names.
    ///
    /// [`secret_name`]: super::redact::secret_name
    /// [`REDACTED`]: super::REDACTED
    pub fn redacted(mut self) -> NetworkEntry {
        self.url = redact_url(&self.url);
        self.request_body = self.request_body.as_deref().map(redact_body);
        self.response_body = self.response_body.as_deref().map(redact_body);
        self
    }
}

/// Console messages and requests since the tab opened (or the last clear).
#[derive(Debug, Default)]
pub struct Logs {
    pub console: VecDeque<ConsoleEntry>,
    pub network: VecDeque<NetworkEntry>,
}

impl Logs {
    pub fn push_console(&mut self, entry: ConsoleEntry) {
        if self.console.len() == MAX_CONSOLE {
            self.console.pop_front();
        }
        self.console.push_back(entry);
    }

    /// Errors from the current page (after its last `— navigated to`
    /// marker): the newest `limit`, oldest first.
    pub fn page_errors(&self, limit: usize) -> Vec<ConsoleEntry> {
        let start = self
            .console
            .iter()
            .rposition(|e| e.text.starts_with("— navigated to"))
            .map_or(0, |i| i + 1);
        let errors: Vec<&ConsoleEntry> = self
            .console
            .iter()
            .skip(start)
            .filter(|e| e.level == "error")
            .collect();
        errors[errors.len().saturating_sub(limit)..]
            .iter()
            .map(|e| (*e).clone())
            .collect()
    }

    /// Keeps `entry`, with its secrets hidden ([`NetworkEntry::redacted`]).
    pub fn push_network(&mut self, entry: NetworkEntry) {
        if self.network.len() == MAX_NETWORK {
            self.network.pop_front();
        }
        self.network.push_back(entry.redacted());
    }

    /// The newest `limit` messages at `level` (`warn` means `warning`)
    /// whose text holds `pattern` (any case), oldest first.
    pub fn console_json(
        &self,
        level: Option<&str>,
        pattern: Option<&str>,
        limit: Option<usize>,
    ) -> Value {
        let level = level.map(|l| match l.trim().to_ascii_lowercase().as_str() {
            "warn" => "warning".to_string(),
            "err" => "error".to_string(),
            other => other.to_string(),
        });
        let pattern = pattern.map(str::to_lowercase).filter(|p| !p.is_empty());
        let matching: Vec<&ConsoleEntry> = self
            .console
            .iter()
            .filter(|e| level.as_deref().is_none_or(|l| e.level == l))
            .filter(|e| {
                pattern
                    .as_deref()
                    .is_none_or(|p| e.text.to_lowercase().contains(p))
            })
            .collect();
        let shown = newest(&matching, limit.unwrap_or(100));
        json!({
            "total": self.console.len(),
            "matching": matching.len(),
            "errors": self.console.iter().filter(|e| e.level == "error").count(),
            "messages": shown,
        })
    }

    pub fn network_json(
        &self,
        pattern: Option<&str>,
        failed_only: bool,
        limit: Option<usize>,
    ) -> Value {
        let pattern = pattern.map(str::to_lowercase).filter(|p| !p.is_empty());
        let matching: Vec<&NetworkEntry> = self
            .network
            .iter()
            .filter(|e| !failed_only || e.failed())
            .filter(|e| {
                pattern
                    .as_deref()
                    .is_none_or(|p| e.url.to_lowercase().contains(p))
            })
            .collect();
        let shown = newest(&matching, limit.unwrap_or(100));
        json!({
            "total": self.network.len(),
            "matching": matching.len(),
            "failed": self.network.iter().filter(|e| e.failed()).count(),
            "requests": shown,
        })
    }
}

fn newest<'a, T>(items: &'a [&'a T], limit: usize) -> &'a [&'a T] {
    &items[items.len().saturating_sub(limit)..]
}
