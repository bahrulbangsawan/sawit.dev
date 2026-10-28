//! Checks GitHub Releases for a newer Sawit, downloads it in the
//! background and offers "Restart for Update", which swaps the .app bundle
//! after the app quits and opens the new one.

use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::Duration;

use gpui_kit::*;
use sawit_core::instances::AppPaths;
use sawit_core::update::{Release, Version, download, latest_release, swap_script};

use crate::rt;

const CHECK_EVERY: Duration = Duration::from_secs(4 * 60 * 60);
/// Let startup finish before touching the network.
const FIRST_CHECK: Duration = Duration::from_secs(5);

#[derive(Clone)]
pub enum UpdateState {
    /// Up to date, or not checked yet.
    Idle,
    Downloading(Version),
    /// Downloaded and unpacked; restarting installs it.
    Ready {
        release: Release,
        app: PathBuf,
    },
    /// Newer, but this copy can't replace itself (a cargo build, or the
    /// release has no app file yet): the button opens the release page.
    Available(Release),
}

pub struct Updater {
    pub state: UpdateState,
    /// The newest release seen, for the changelog page.
    pub latest: Option<Release>,
    /// The running `Sawit.app`, when launched from one.
    bundle: Option<PathBuf>,
}

struct GlobalUpdater(Entity<Updater>);

impl Global for GlobalUpdater {}

pub fn updater(cx: &App) -> Entity<Updater> {
    cx.global::<GlobalUpdater>().0.clone()
}

pub fn init(cx: &mut App) {
    let bundle = std::env::current_exe()
        .and_then(|e| e.canonicalize())
        .ok()
        .and_then(|exe| crate::cli::bundle_of(&exe).map(PathBuf::from));
    let updater = cx.new(|cx| {
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(FIRST_CHECK).await;
            loop {
                if this.update(cx, |u: &mut Updater, cx| u.check(cx)).is_err() {
                    return;
                }
                cx.background_executor().timer(CHECK_EVERY).await;
            }
        })
        .detach();
        Updater {
            state: UpdateState::Idle,
            latest: None,
            bundle,
        }
    });
    cx.set_global(GlobalUpdater(updater));
}

impl Updater {
    pub fn check(&mut self, cx: &mut Context<Self>) {
        if matches!(self.state, UpdateState::Downloading(_)) {
            return;
        }
        let rx = rt::spawn(cx, latest_release());
        cx.spawn(async move |this, cx| {
            let Ok(Ok(release)) = rx.await else { return };
            let _ = this.update(cx, |this, cx| this.found(release, cx));
        })
        .detach();
    }

    fn found(&mut self, release: Release, cx: &mut Context<Self>) {
        self.latest = Some(release.clone());
        cx.notify();
        if release.version <= Version::current() {
            return;
        }
        if let UpdateState::Ready { release: r, .. } = &self.state
            && r.version == release.version
        {
            return;
        }
        if self.bundle.is_none() || release.asset.is_none() {
            self.state = UpdateState::Available(release);
            return;
        }
        self.state = UpdateState::Downloading(release.version);
        let dir = AppPaths::default_macos()
            .cache_dir()
            .join("update")
            .join(release.version.to_string());
        let rx = rt::spawn(cx, {
            let release = release.clone();
            async move { download(&release, &dir).await }
        });
        cx.spawn(async move |this, cx| {
            let result = rx.await;
            let _ = this.update(cx, |this, cx| {
                this.state = match result {
                    Ok(Ok(app)) => UpdateState::Ready { release, app },
                    // Fall back to the release page; the next check retries.
                    _ => UpdateState::Available(release),
                };
                cx.notify();
            });
        })
        .detach();
    }

    /// Newer than this build and ready to act on.
    pub fn pending(&self) -> Option<&Release> {
        match &self.state {
            UpdateState::Ready { release, .. } | UpdateState::Available(release) => Some(release),
            _ => None,
        }
    }

    /// Installs the downloaded app and relaunches, or opens the release page.
    pub fn restart(&mut self, cx: &mut Context<Self>) {
        match &self.state {
            UpdateState::Ready { app, .. } => {
                let Some(bundle) = &self.bundle else { return };
                let script = swap_script(std::process::id(), bundle, app);
                let spawned = Command::new("/bin/sh")
                    .arg("-c")
                    .arg(script)
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .process_group(0)
                    .spawn();
                if spawned.is_ok() {
                    cx.quit();
                }
            }
            UpdateState::Available(release) => cx.open_url(&release.page),
            _ => {}
        }
    }
}
