//! curl's command lines, for a download or a small text fetch, and what curl reports after
//! each.

use std::ffi::OsString;
use std::path::Path;
use std::time::Duration;

use crate::{CoreError, cmd};

pub const SAFARI_UA: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.0 Safari/605.1.15";

pub(super) fn curl_args_with(
    url: &str,
    out: Option<&Path>,
    referer: &str,
    max_time: &str,
    max_size: &str,
) -> Vec<OsString> {
    // No `--compressed`: `--max-filesize` counts the bytes on the wire, so curl would
    // inflate a gzip answer far past it (100 KB of gzip can hold 100 MB).
    // `-q` (first, or curl ignores it): no `~/.curlrc`, whose headers, cookies or user
    // would go to whatever host a page lists.
    let mut a: Vec<OsString> = ["-q", "-fsSL", "--max-time", max_time]
        .into_iter()
        .map(OsString::from)
        .collect();
    a.extend(
        [
            "--proto",
            "=http,https",
            "--proto-redir",
            "=http,https",
            "--max-filesize",
            max_size,
            "-A",
            SAFARI_UA,
            "-e",
            referer,
        ]
        .map(OsString::from),
    );
    if let Some(out) = out {
        a.push("-o".into());
        a.push(out.as_os_str().to_os_string());
    }
    a.push("--".into());
    a.push(url.into());
    a
}

/// A download of `url` into `out`, of at most `max_size` bytes.
pub fn curl_args(url: &str, out: &Path, referer: &str, max_size: u64) -> Vec<OsString> {
    curl_args_with(url, Some(out), referer, "30", &max_size.to_string())
}

/// A fetched text resource, and what curl says about where it came from.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Fetched {
    pub text: String,
    /// The response's `Content-Type`, as sent.
    pub content_type: String,
    /// Where redirects ended.
    pub final_url: String,
    /// The address curl connected to last.
    pub remote_ip: String,
}

/// What curl writes after the body: one value per line (none can hold a newline).
const WRITE_OUT: &str = "\n%{content_type}\n%{url_effective}\n%{remote_ip}";

/// Fetches a small text resource (cross-origin CSS, manifests).
pub async fn fetch_text(url: &str, referer: &str) -> crate::Result<Fetched> {
    let mut args = curl_args_with(url, None, referer, "10", "5242880");
    // Before `--`: the URL stays last.
    let at = args.len() - 2;
    args.splice(at..at, ["-w".into(), OsString::from(WRITE_OUT)]);
    let out = cmd::run("/usr/bin/curl", args, Duration::from_secs(15)).await?;
    if out.code != Some(0) {
        return Err(CoreError::Download(out.stderr.trim().to_string()));
    }
    parse_fetched(&out.stdout)
        .ok_or_else(|| CoreError::Download("curl's answer couldn't be read.".into()))
}

/// The body and [`WRITE_OUT`]'s lines after it.
pub(super) fn parse_fetched(stdout: &str) -> Option<Fetched> {
    let mut parts = stdout.rsplitn(4, '\n');
    let remote_ip = parts.next()?.trim().to_string();
    let final_url = parts.next()?.trim().to_string();
    let content_type = parts.next()?.trim().to_string();
    let text = parts.next()?.to_string();
    Some(Fetched {
        text,
        content_type,
        final_url,
        remote_ip,
    })
}

/// What curl writes after a download (to stdout, the file going to `-o`). `proxy_used` is
/// curl 8.7's (macOS 15); an older curl writes nothing for it.
pub(super) const DOWNLOAD_OUT: &str = "%{url_effective}\n%{remote_ip}\n%{proxy_used}";

/// Where a download ended, the address curl got it from and whether through a proxy, from
/// what curl wrote after it ([`DOWNLOAD_OUT`]).
pub(super) fn download_report(stdout: &str) -> (&str, &str, bool) {
    let mut lines = stdout.lines().map(str::trim);
    let final_url = lines.next().unwrap_or("");
    let remote_ip = lines.next().unwrap_or("");
    let proxied = lines.next() == Some("1");
    (final_url, remote_ip, proxied)
}
