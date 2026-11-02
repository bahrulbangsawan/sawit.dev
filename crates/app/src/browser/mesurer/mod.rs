//! mesurer on the page while Annotate is open (`sawit_core::browser::mesurer`): the same switch,
//! one list. Opening Annotate mounts it, once per page load: its bundle and Sawit's glue in the
//! `sawit-mesurer` content world (`engine/world.rs`), `SawitMesurer.mount` with the settings and
//! workspace read from Sawit's files, then its keyboard gate in the page's world. A new page mounts
//! it again while Annotate stays open; closing Annotate, or opening Design, unmounts it.
//!
//! What the glue posts is answered in `receive.rs` (the foundation's kinds) and in the lanes'
//! files: `capture.rs` (lane 1), `agents.rs` (lane 2), `keys.rs` (lane 3). Agents' `mesurer` op is
//! in `op.rs`, the store in `store.rs`.

mod agents;
mod capture;
mod keys;
mod op;
mod receive;
mod store;

use std::path::PathBuf;
use std::time::Duration;

use futures::channel::oneshot;
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::*;
use sawit_core::browser::mesurer::{self as core, Summary, calls};
use serde_json::{Value, json};

use super::shots::within;
use super::{BrowserView, Panel};

/// How long one call into mesurer's world may take before it is given up: a page that hangs
/// mustn't keep a mount or a screenshot waiting.
pub(super) const CALL_TIMEOUT: Duration = Duration::from_secs(10);

/// What the browser knows of mesurer on its page.
#[derive(Default)]
pub(super) struct MesurerState {
    /// mesurer is mounted on the page on show.
    pub mounted: bool,
    /// Counts the page's loads: a step of a mount begun before the last one is dropped.
    pub load_gen: u64,
    /// The load whose world has the bundle and the glue.
    pub injected_gen: Option<u64>,
    /// The load a mount is under way for.
    mounting: Option<u64>,
    pub store: store::Cache,
    /// What mesurer has on the page on show, as last saved.
    pub summary: Summary,
    /// Screenshots and recordings mesurer saved this session (lane 1).
    pub captures: Vec<PathBuf>,
    /// What mesurer last copied, kept instead of the pasteboard under `SAWIT_E2E_RUN`.
    pub last_clipboard: Option<String>,
    /// What `ready` said last: `{version, url, mount_ms}`.
    pub ready: Option<Value>,
    /// The lanes' own state, which their files read and write.
    #[allow(dead_code)]
    pub capture: capture::CaptureState,
    #[allow(dead_code)]
    pub agents: agents::AgentsState,
    #[allow(dead_code)]
    pub keys: keys::KeysState,
}

/// Whether Sawit runs under its e2e harness or a scratch run (`SAWIT_E2E_RUN`): mesurer's copies
/// then go to `last_clipboard`, never to the user's pasteboard.
pub(super) fn e2e_run() -> bool {
    std::env::var_os("SAWIT_E2E_RUN").is_some_and(|v| !v.is_empty())
}

/// Whether mesurer is put on a page at `url`: not on `about:` pages (the blank tab).
fn mountable(url: &str) -> bool {
    !url.is_empty() && !url.starts_with("about:")
}

/// Sawit's theme, as mesurer names it.
fn theme(cx: &App) -> &'static str {
    if cx.theme().mode.is_dark() {
        "dark"
    } else {
        "light"
    }
}

impl BrowserView {
    /// Annotate opened: mesurer is mounted on the page now, or once it has loaded.
    pub(super) fn mesurer_open(&mut self, cx: &mut Context<Self>) {
        if !self.loading {
            self.mesurer_mount(cx);
        }
    }

    /// Annotate closed, or Design opened: mesurer is unmounted (it saves its workspace first),
    /// and its files are written.
    pub(super) fn mesurer_close(&mut self, cx: &mut Context<Self>) {
        let was = self.mesurer.mounted || self.mesurer.mounting.is_some();
        self.mesurer.mounted = false;
        self.mesurer.mounting = None;
        self.annotate
            .update(cx, |a, cx| a.set_mesurer_summary(None, cx));
        if !was {
            return;
        }
        let unmounted = self.world_json(calls::UNMOUNT, "null".into());
        cx.spawn(async move |this, cx| {
            let timer = cx.background_executor().timer(CALL_TIMEOUT);
            if let Some(Ok(Err(e))) = within(unmounted, timer).await {
                eprintln!("sawit: mesurer couldn't be unmounted: {e}");
            }
            // mesurer's last save is in by now.
            let _ = this.update(cx, |this, cx| this.mesurer.store.flush(cx));
        })
        .detach();
        cx.notify();
    }

    /// A page starts loading: its world is a new one, with no mesurer in it.
    pub(super) fn mesurer_load_started(&mut self, cx: &mut Context<Self>) {
        let state = &mut self.mesurer;
        state.load_gen += 1;
        state.mounted = false;
        state.mounting = None;
        state.injected_gen = None;
        state.ready = None;
        state.store.flush(cx);
    }

    /// A page has loaded: mesurer is mounted on it while Annotate is open.
    pub(super) fn mesurer_after_load(&mut self, cx: &mut Context<Self>) {
        if self.panel == Some(Panel::Annotate) {
            self.mesurer_mount(cx);
        }
    }

    /// Whether a mount begun for load `generation` is still wanted.
    fn mount_wanted(&self, generation: u64) -> bool {
        self.panel == Some(Panel::Annotate)
            && self.mesurer.load_gen == generation
            && self.mesurer.mounting == Some(generation)
    }

    /// Mounts mesurer on the page: the bundle and the glue in mesurer's world (once per load),
    /// the files read off the main thread, `SawitMesurer.mount`, then the gate in the page's world.
    fn mesurer_mount(&mut self, cx: &mut Context<Self>) {
        if self.engine.is_none() {
            return;
        }
        let url = self.current_url();
        let generation = self.mesurer.load_gen;
        if !mountable(&url) || self.mesurer.mounted || self.mesurer.mounting == Some(generation) {
            return;
        }
        self.mesurer.mounting = Some(generation);
        let origin = core::origin_key(&url);
        let cached = self.mesurer.store.snapshot(&origin);
        let project = self.project.clone();
        let props = json!({ "theme": theme(cx) });
        cx.spawn(async move |this, cx| {
            let failed = |this: &WeakEntity<Self>, cx: &mut AsyncApp, why: String| {
                eprintln!("sawit: mesurer couldn't be mounted: {why}");
                let _ = this.update(cx, |this, _| {
                    if this.mesurer.mounting == Some(generation) {
                        this.mesurer.mounting = None;
                    }
                });
            };
            // The bundle and the glue, unless this page's world has them.
            let Ok(loaded) =
                this.update(cx, |this, _| this.world_json(calls::LOADED, "null".into()))
            else {
                return;
            };
            let timer = cx.background_executor().timer(CALL_TIMEOUT);
            let loaded = match within(loaded, timer).await {
                Some(Ok(Ok(loaded))) => loaded == Value::Bool(true),
                Some(Ok(Err(e))) => return failed(&this, cx, e),
                _ => return failed(&this, cx, "the page didn't answer".into()),
            };
            if !loaded {
                let Ok(evaluated) = this.update(cx, |this, _| this.world_load()) else {
                    return;
                };
                let timer = cx.background_executor().timer(CALL_TIMEOUT);
                match within(evaluated, timer).await {
                    Some(Ok(Ok(()))) => {}
                    Some(Ok(Err(e))) => return failed(&this, cx, e),
                    _ => return failed(&this, cx, "the page didn't take mesurer".into()),
                }
            }
            let _ = this.update(cx, |this, _| {
                if this.mesurer.load_gen == generation {
                    this.mesurer.injected_gen = Some(generation);
                }
            });
            // The files, read off the main thread unless they are held already.
            let snapshot = match cached {
                Some(snapshot) => snapshot,
                None => {
                    let origin = origin.clone();
                    let read = cx.update(|cx| {
                        crate::rt::spawn_blocking(cx, move || {
                            store::read_snapshot(project.as_deref(), &origin)
                        })
                    });
                    read.await.unwrap_or_else(|_| json!({}))
                }
            };
            let Ok(Some(mounted)) = this.update(cx, |this, cx| {
                if !this.mount_wanted(generation) {
                    return None;
                }
                this.mesurer.store.loaded(&origin, &snapshot);
                this.mesurer_summary_changed(cx);
                let args = core::mount_args(&props, &snapshot);
                Some(this.world_json(calls::MOUNT, args))
            }) else {
                let _ = this.update(cx, |this, _| {
                    if this.mesurer.mounting == Some(generation) {
                        this.mesurer.mounting = None;
                    }
                });
                return;
            };
            let timer = cx.background_executor().timer(CALL_TIMEOUT);
            match within(mounted, timer).await {
                Some(Ok(Ok(_))) => {
                    let _ = this.update(cx, |this, cx| {
                        let wanted = this.mount_wanted(generation);
                        this.mesurer.mounting = None;
                        if wanted {
                            this.mesurer.mounted = true;
                            // The gate after mesurer (mesurer's extension puts it first): once
                            // mesurer has the keyboard, the gate stops every key but the ones
                            // it bridges, which leaves out L and B; mesurer's own listeners,
                            // there first on the window, still see them. On a mount again in
                            // the same page the gate is there first (lane 3).
                            if let Some(engine) = &this.engine {
                                engine.eval(core::MESURER_GATE_JS);
                            }
                        } else if this.mesurer.load_gen == generation {
                            // Annotate closed meanwhile.
                            drop(this.world_json(calls::UNMOUNT, "null".into()));
                        }
                        cx.notify();
                    });
                }
                Some(Ok(Err(e))) => failed(&this, cx, e),
                _ => failed(&this, cx, "SawitMesurer.mount didn't answer".into()),
            }
        })
        .detach();
    }

    /// The page's summary, from the workspace held, to the Annotate panel.
    pub(super) fn mesurer_summary_changed(&mut self, cx: &mut Context<Self>) {
        let url = self.current_url();
        let origin = core::origin_key(&url);
        let summary = self
            .mesurer
            .store
            .workspace(&origin)
            .map(|envelope| Summary::of(envelope, &core::page_key(&url)))
            .unwrap_or_default();
        self.mesurer.summary = summary.clone();
        self.annotate
            .update(cx, |a, cx| a.set_mesurer_summary(Some(summary), cx));
        // Comment threads as rows, and the markup's elements (lane 2).
        agents::workspace_changed(self, cx);
    }

    /// Runs a `callAsyncJavaScript` body ([`calls`]) in mesurer's world with `json` as its
    /// argument; the receiver gets the JSON value it answered.
    pub(super) fn world_json(
        &self,
        body: &'static str,
        json: String,
    ) -> oneshot::Receiver<Result<Value, String>> {
        let (tx, rx) = oneshot::channel();
        let Some(engine) = &self.engine else {
            let _ = tx.send(Err("The browser isn't open.".into()));
            return rx;
        };
        engine.world_call(body, &json, move |result| {
            let value = result.and_then(|text| match text {
                Some(text) => serde_json::from_str(&text)
                    .map_err(|e| format!("mesurer's world answered bad JSON: {e}")),
                None => Ok(Value::Null),
            });
            let _ = tx.send(value);
        });
        rx
    }

    /// Evaluates mesurer's bundle and Sawit's glue in mesurer's world.
    fn world_load(&self) -> oneshot::Receiver<Result<(), String>> {
        let (tx, rx) = oneshot::channel();
        let Some(engine) = &self.engine else {
            let _ = tx.send(Err("The browser isn't open.".into()));
            return rx;
        };
        engine.world_eval(&core::world_script(), move |result| {
            let _ = tx.send(result.map(|_| ()));
        });
        rx
    }

    /// Hides mesurer (`all`, `chrome`) or shows it (`none`); `None` while it isn't mounted.
    pub(super) fn mesurer_hide(
        &self,
        mode: &str,
    ) -> Option<oneshot::Receiver<Result<Value, String>>> {
        self.mesurer
            .mounted
            .then(|| self.world_json(calls::HIDE, Value::from(mode).to_string()))
    }
}

/// What mesurer is set to around a screenshot: hidden, then shown again; nothing while it isn't
/// mounted.
pub(super) fn shot_hides(mounted: bool) -> Option<[&'static str; 2]> {
    mounted.then_some(["all", "none"])
}
