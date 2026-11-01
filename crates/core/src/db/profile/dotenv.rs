//! `DATABASE_URL` and other keys in `.env` text.

use std::path::Path;

use crate::Result;

/// Value of `key` in .env text (`KEY=v`, `export KEY="v"`, `KEY='v'`, with
/// trailing `# comments` on unquoted values).
pub fn dotenv_value(text: &str, key: &str) -> Option<String> {
    text.lines().find_map(|line| {
        let line = line.trim();
        let line = line.strip_prefix("export ").unwrap_or(line).trim_start();
        let (k, v) = line.split_once('=')?;
        if k.trim() != key {
            return None;
        }
        let v = v.trim();
        let value = if let Some(q) = v.strip_prefix('"') {
            q.split_once('"').map(|(inner, _)| inner.to_string())?
        } else if let Some(q) = v.strip_prefix('\'') {
            q.split_once('\'').map(|(inner, _)| inner.to_string())?
        } else {
            v.split(" #").next().unwrap_or_default().trim().to_string()
        };
        Some(value)
    })
}

/// Sets `key=value` in .env text: replaces the first active assignment or appends.
pub fn dotenv_upsert(text: &str, key: &str, value: &str) -> String {
    let mut lines: Vec<String> = text.lines().map(str::to_string).collect();
    let is_key = |l: &str| {
        let l = l.trim();
        let l = l.strip_prefix("export ").unwrap_or(l).trim_start();
        l.split_once('=').is_some_and(|(k, _)| k.trim() == key)
    };
    match lines.iter().position(|l| is_key(l)) {
        Some(i) => lines[i] = format!("{key}={value}"),
        None => lines.push(format!("{key}={value}")),
    }
    let mut out = lines.join("\n");
    out.push('\n');
    out
}

/// Reads `DATABASE_URL` from a .env file.
pub fn read_dotenv_database_url(path: &Path) -> Result<Option<String>> {
    Ok(dotenv_value(
        &std::fs::read_to_string(path)?,
        "DATABASE_URL",
    ))
}
