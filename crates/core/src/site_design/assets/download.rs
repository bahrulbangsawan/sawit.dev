//! Saving a page's assets into the export's folder under one size budget: inline data written,
//! a `file://` page's files copied, the rest downloaded with curl, six at a time.

use std::collections::HashSet;
use std::ffi::OsString;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use futures::StreamExt;

use super::curl::{DOWNLOAD_OUT, curl_args, download_report};
use super::data_url::{decode_data_url, percent_decode};
use super::hosts::local_page;
use super::naming::{file_name, names_in, sub_dir};
use super::policy::{download_allowed, may_download};
use crate::cmd;
use crate::site_design::scan::{AssetKind, AssetOutcome, AssetRef};

pub const MAX_TOTAL: u64 = 300 * 1024 * 1024;

pub(super) const BLOB_MSG: &str = "Can't save blob: media (made by the page's scripts).";
/// A video left out because videos weren't asked for: skipped, not failed.
pub const VIDEO_SKIPPED: &str = "Videos weren't included.";

fn outcome(a: &AssetRef, file: Option<String>, bytes: u64, error: Option<String>) -> AssetOutcome {
    AssetOutcome {
        file,
        url: a.url.clone(),
        kind: a.kind,
        bytes,
        width: a.width,
        height: a.height,
        label: a.label.clone(),
        used_on: a.used_on.clone(),
        error,
    }
}

fn budget_msg(budget: u64) -> String {
    format!(
        "Skipped: the {} MB export limit was reached.",
        budget / (1024 * 1024)
    )
}

fn write_local(
    a: &AssetRef,
    dir: &Path,
    taken: &mut HashSet<String>,
    total: &AtomicU64,
) -> AssetOutcome {
    let bytes = if let Some(svg) = &a.svg {
        svg.as_bytes().to_vec()
    } else {
        match a.url.as_deref().and_then(decode_data_url).map(|(_, b)| b) {
            Some(b) => b,
            None => return outcome(a, None, 0, Some("Couldn't read the inline data.".into())),
        }
    };
    let sub = sub_dir(a.kind);
    let name = file_name(a, taken);
    let target = dir.join(sub);
    let res =
        std::fs::create_dir_all(&target).and_then(|_| std::fs::write(target.join(&name), &bytes));
    match res {
        Ok(()) => {
            total.fetch_add(bytes.len() as u64, Ordering::SeqCst);
            outcome(a, Some(format!("{sub}/{name}")), bytes.len() as u64, None)
        }
        Err(e) => outcome(a, None, 0, Some(e.to_string())),
    }
}

const MAX_FILE: u64 = 52_428_800;

/// Copies a `file://` asset of a `file://` page, without curl.
fn copy_file_url(
    a: &AssetRef,
    url: &str,
    dir: &Path,
    taken: &mut HashSet<String>,
    total: &AtomicU64,
    budget: u64,
) -> AssetOutcome {
    if total.load(Ordering::SeqCst) >= budget {
        return outcome(a, None, 0, Some(budget_msg(budget)));
    }
    let rest = url.strip_prefix("file://").unwrap_or("");
    let rest = rest.strip_prefix("localhost").unwrap_or(rest);
    let rest = rest.split(['?', '#']).next().unwrap_or("");
    let src = std::path::PathBuf::from(
        String::from_utf8_lossy(&percent_decode(rest.as_bytes())).into_owned(),
    );
    let len = match std::fs::metadata(&src) {
        Ok(m) if m.is_file() => m.len(),
        Ok(_) => return outcome(a, None, 0, Some("Not a file.".into())),
        Err(e) => return outcome(a, None, 0, Some(e.to_string())),
    };
    if len > MAX_FILE {
        return outcome(a, None, 0, Some("The file is larger than 50 MB.".into()));
    }
    let sub = sub_dir(a.kind);
    let name = file_name(a, taken);
    let target = dir.join(sub);
    let res =
        std::fs::create_dir_all(&target).and_then(|_| std::fs::copy(&src, target.join(&name)));
    match res {
        Ok(n) => {
            total.fetch_add(n, Ordering::SeqCst);
            outcome(a, Some(format!("{sub}/{name}")), n, None)
        }
        Err(e) => outcome(a, None, 0, Some(e.to_string())),
    }
}

/// Takes up to `most` bytes of what is left of `budget` (`total` is what is taken), so
/// downloads running at once can't pass the budget between them; `None` when none is left.
fn claim(total: &AtomicU64, budget: u64, most: u64) -> Option<u64> {
    let mut used = total.load(Ordering::SeqCst);
    loop {
        let take = budget.saturating_sub(used).min(most);
        if take == 0 {
            return None;
        }
        match total.compare_exchange(used, used + take, Ordering::SeqCst, Ordering::SeqCst) {
            Ok(_) => return Some(take),
            Err(now) => used = now,
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn fetch_one(
    a: &AssetRef,
    url: &str,
    rel: String,
    dir: &Path,
    referer: &str,
    local_page: bool,
    total: &AtomicU64,
    budget: u64,
) -> AssetOutcome {
    // The file may be as large as what this download holds of the budget, and gives
    // back what it didn't use.
    let Some(held) = claim(total, budget, MAX_FILE) else {
        return outcome(a, None, 0, Some(budget_msg(budget)));
    };
    let path = dir.join(&rel);
    if let Some(parent) = path.parent()
        && let Err(e) = std::fs::create_dir_all(parent)
    {
        total.fetch_sub(held, Ordering::SeqCst);
        return outcome(a, None, 0, Some(e.to_string()));
    }
    let mut args = curl_args(url, &path, referer, held);
    // Before `--`: the URL stays last.
    let at = args.len() - 2;
    args.splice(at..at, ["-w".into(), OsString::from(DOWNLOAD_OUT)]);
    let res = cmd::run("/usr/bin/curl", args, Duration::from_secs(40)).await;
    let err = match res {
        // Redirects may have ended on this Mac or its network.
        Ok(o) if o.code == Some(0) => {
            let (final_url, ip, proxied) = download_report(&o.stdout);
            download_allowed(url, final_url, ip, proxied, local_page)
                .err()
                .map(|why| format!("Left out: {why}."))
        }
        Ok(o) => Some(o.stderr.trim().to_string()),
        Err(e) => Some(e.to_string()),
    };
    if let Some(msg) = err {
        let _ = std::fs::remove_file(&path);
        total.fetch_sub(held, Ordering::SeqCst);
        let msg = if msg.is_empty() {
            "Download failed.".into()
        } else {
            msg
        };
        return outcome(a, None, 0, Some(msg));
    }
    let len = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
    total.fetch_sub(held - len.min(held), Ordering::SeqCst);
    outcome(a, Some(rel), len, None)
}

pub(super) fn is_file_url(url: &str) -> bool {
    url.trim().to_ascii_lowercase().starts_with("file://")
}

/// Whether `o` is a video left out because videos weren't asked for.
pub fn is_skipped_video(o: &AssetOutcome) -> bool {
    o.kind == AssetKind::Video && o.error.as_deref() == Some(VIDEO_SKIPPED)
}

/// Downloads `items` into `assets_dir`. `page` is the tab's URL by Sawit's word (not the
/// page's, which `referer` may be): only for a `file://` page are `file://` assets copied, and
/// what a download may fetch and keep follows where the page is ([`local_page`], looked up
/// once something is to be downloaded; [`download_allowed`]).
pub async fn download_all(
    items: Vec<AssetRef>,
    assets_dir: &Path,
    referer: &str,
    page: &str,
    include_videos: bool,
    budget: u64,
    progress: impl Fn(usize, usize) + Send + Sync,
) -> Vec<AssetOutcome> {
    download_for(
        items,
        assets_dir,
        referer,
        page,
        None,
        include_videos,
        budget,
        progress,
    )
    .await
}

/// [`download_all`], with whether the page is on this Mac or its network known
/// (`page_here`) or, when `None`, looked up the first time it matters.
#[allow(clippy::too_many_arguments)]
pub(super) async fn download_for(
    items: Vec<AssetRef>,
    assets_dir: &Path,
    referer: &str,
    page: &str,
    mut page_here: Option<bool>,
    include_videos: bool,
    budget: u64,
    progress: impl Fn(usize, usize) + Send + Sync,
) -> Vec<AssetOutcome> {
    let file_page = is_file_url(page);
    let n = items.len();
    let mut results: Vec<Option<AssetOutcome>> = vec![None; n];
    // An earlier Save's files keep theirs: new files take the next free name.
    let mut taken = names_in(assets_dir);
    let total = AtomicU64::new(0);
    let mut done = 0;
    let mut remote: Vec<(usize, String, String)> = Vec::new();

    for (i, a) in items.iter().enumerate() {
        let url = a.url.as_deref();
        let local = a.svg.is_some() || url.is_some_and(|u| u.starts_with("data:"));
        let res = if url.is_some_and(|u| u.starts_with("blob:")) {
            Some(outcome(a, None, 0, Some(BLOB_MSG.into())))
        } else if a.kind == AssetKind::Video && !include_videos {
            Some(outcome(a, None, 0, Some(VIDEO_SKIPPED.into())))
        } else if local {
            if total.load(Ordering::SeqCst) >= budget {
                Some(outcome(a, None, 0, Some(budget_msg(budget))))
            } else {
                Some(write_local(a, assets_dir, &mut taken, &total))
            }
        } else if let Some(u) = url.filter(|u| u.starts_with("file://")) {
            if file_page {
                Some(copy_file_url(a, u, assets_dir, &mut taken, &total, budget))
            } else {
                Some(outcome(
                    a,
                    None,
                    0,
                    Some("Local files can only be saved from a local page.".into()),
                ))
            }
        } else if let Some(u) = url.filter(|u| {
            let l = u.to_ascii_lowercase();
            !(l.starts_with("http://") || l.starts_with("https://"))
        }) {
            let scheme = u.split(':').next().unwrap_or("");
            Some(outcome(
                a,
                None,
                0,
                Some(format!("Can't download {scheme}: URLs.")),
            ))
        } else if let Some(u) = url {
            let here = match page_here {
                Some(known) => known,
                None => *page_here.insert(local_page(page).await),
            };
            if here || may_download(u) {
                let rel = format!("{}/{}", sub_dir(a.kind), file_name(a, &mut taken));
                remote.push((i, u.to_string(), rel));
                None
            } else {
                Some(outcome(
                    a,
                    None,
                    0,
                    Some("Left out: it is on this Mac or its network.".into()),
                ))
            }
        } else {
            Some(outcome(a, None, 0, Some("No source to download.".into())))
        };
        if let Some(r) = res {
            results[i] = Some(r);
            done += 1;
            progress(done, n);
        }
    }

    // Known by now whenever there is something to download.
    let here = page_here.unwrap_or(false);
    let items_ref = &items;
    let total_ref = &total;
    let mut stream = futures::stream::iter(remote.into_iter().map(|(i, url, rel)| async move {
        let o = fetch_one(
            &items_ref[i],
            &url,
            rel,
            assets_dir,
            referer,
            here,
            total_ref,
            budget,
        )
        .await;
        (i, o)
    }))
    .buffer_unordered(6);
    while let Some((i, o)) = stream.next().await {
        results[i] = Some(o);
        done += 1;
        progress(done, n);
    }
    results.into_iter().flatten().collect()
}
