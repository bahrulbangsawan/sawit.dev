//! Command-line tools from Homebrew formulas: the GitHub CLI, MinIO and its client, FFmpeg.
//! Each copy found in Homebrew's Cellar is upgraded with `brew upgrade` of the formula its
//! install receipt names, and removed with `brew uninstall`; a copy installed another way is
//! detected and left alone ([`Tool::formula`]).

use super::super::{Category, Latest, Tool};
use super::tool;

/// The GitHub CLI, from Homebrew's core (`brew info gh`: homebrew/core, bottled). Its `.pkg`
/// from cli.github.com puts `gh` in /usr/local/bin, MacPorts in /opt/local/bin, conda in its
/// environment's bin: those copies are found and never upgraded or removed by Sawit, as they
/// aren't in Homebrew's Cellar. `gh --version` prints "gh version 2.103.3 (2026-10-26)" and
/// the release's URL. Signing in is `gh auth login`, which asks questions, so it runs in a
/// terminal; Sawit's own GitHub sign-in is separate, and its token is never copied into gh.
pub(super) const GH: Tool = Tool {
    formula: Some("gh"),
    latest: Latest::GitHub("cli/cli"),
    upgrade: Some("brew upgrade gh"),
    uninstall: Some("brew uninstall gh"),
    needs: &["homebrew"],
    ..tool(
        "gh",
        "GitHub CLI",
        "GitHub from the terminal: pull requests, issues, releases and Actions with gh.",
        Category::Other,
        "gh",
        "brew install gh",
    )
};

/// MinIO's S3-compatible server, from MinIO's own tap `minio/stable`
/// (github.com/minio/homebrew-stable: `minio.rb`, `mc.rb` and `kes.rb`, each downloading
/// MinIO's release binary from dl.min.io). Homebrew's core has `minio` and `minio-mc` too,
/// deprecated since MinIO archived their repositories (core disables `minio` on 2027-02-17 and
/// `minio-mc` on 2027-07-17); a copy from there is Homebrew's as well, and `brew upgrade` names
/// the formula it came from. `minio --version` prints "minio version
/// RELEASE.2025-10-15T17-29-55Z (commit-id=…)". With the repositories archived no release is
/// coming, so no latest is fetched: `brew upgrade` brings what the formula has. A server that
/// runs keeps running until it is stopped, and its data folders are the user's.
pub(super) const MINIO: Tool = Tool {
    formula: Some("minio/stable/minio"),
    signature: Some("minio"),
    upgrade: Some("brew upgrade minio/stable/minio"),
    uninstall: Some("brew uninstall minio"),
    needs: &["homebrew"],
    warning: Some(
        "A MinIO server that is running keeps running until you stop it. Its data folders are left in place.",
    ),
    ..tool(
        "minio",
        "MinIO",
        "S3-compatible object storage server, from MinIO's Homebrew tap.",
        Category::Other,
        "minio",
        "brew install minio/stable/minio",
    )
};

/// The MinIO Client, from MinIO's tap (`minio/stable/mc`, which conflicts with Midnight
/// Commander). Homebrew's core `mc` is Midnight Commander (an alias of `midnight-commander`),
/// and core's MinIO Client is `minio-mc`, deprecated as above. An `mc` counts only when
/// `mc --version` names MinIO ("mc version RELEASE.2025-08-13T08-35-41Z (commit-id=…)",
/// "Copyright (c) 2015-2025 MinIO, Inc."); Midnight Commander's says "GNU Midnight
/// Commander". Its aliases and credentials live in ~/.mc, which uninstalling leaves.
pub(super) const MC: Tool = Tool {
    formula: Some("minio/stable/mc"),
    signature: Some("minio"),
    upgrade: Some("brew upgrade minio/stable/mc"),
    uninstall: Some("brew uninstall mc"),
    needs: &["homebrew"],
    keeps: &["~/.mc"],
    ..tool(
        "mc",
        "MinIO Client",
        "mc: buckets and objects on MinIO, S3 and other object storage, from MinIO's tap.",
        Category::Other,
        "mc",
        "brew install minio/stable/mc",
    )
};

/// FFmpeg from Homebrew's core, with `ffprobe`: what HyperFrames encodes video and probes
/// media with (`hyperframes doctor` asks for both). `ffmpeg -version` prints "ffmpeg version
/// 8.0 Copyright …"; `--version` isn't one of its options.
pub(super) const FFMPEG: Tool = Tool {
    formula: Some("ffmpeg"),
    version_arg: "-version",
    upgrade: Some("brew upgrade ffmpeg"),
    uninstall: Some("brew uninstall ffmpeg"),
    needs: &["homebrew"],
    ..tool(
        "ffmpeg",
        "FFmpeg",
        "Records, converts and streams audio and video; ffprobe reads media files.",
        Category::Other,
        "ffmpeg",
        "brew install ffmpeg",
    )
};
