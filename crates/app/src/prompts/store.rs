//! The prompt library shared by the sidebar and the prompt tabs; writes to
//! disk shortly after the last change.

use std::path::PathBuf;
use std::time::Duration;

use gpui_kit::{Context, EventEmitter};
use sawit_core::instances::AppPaths;
use sawit_core::prompts::{Library, Prompt, import_whatsapp, now};

use crate::rt;

/// Wait this long after the last edit before writing the file.
const SAVE_DEBOUNCE: Duration = Duration::from_millis(400);

pub enum PromptStoreEvent {
    /// A prompt was deleted (its tab should close).
    Removed(String),
}

impl EventEmitter<PromptStoreEvent> for PromptStore {}

pub struct PromptStore {
    file: PathBuf,
    pub library: Library,
    /// Set when the file exists but can't be read: nothing is written then,
    /// so a damaged file is never replaced by an empty library.
    pub load_error: Option<String>,
    pub notice: Option<String>,
    save_scheduled: bool,
}

impl PromptStore {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let file = AppPaths::default_macos().root.join("prompts.json");
        let mut this = Self {
            file: file.clone(),
            library: Library::default(),
            load_error: None,
            notice: None,
            save_scheduled: false,
        };
        let rx = rt::spawn(cx, async move { Library::load(&file) });
        cx.spawn(async move |this, cx| {
            let result = rx.await;
            let _ = this.update(cx, |this, cx| {
                match result {
                    Ok(Ok(library)) => this.library = library,
                    Ok(Err(e)) => this.load_error = Some(e.to_string()),
                    Err(_) => {}
                }
                cx.notify();
            });
        })
        .detach();
        this.save_scheduled = false;
        this
    }

    fn changed(&mut self, cx: &mut Context<Self>) {
        cx.notify();
        if self.load_error.is_some() || self.save_scheduled {
            return;
        }
        self.save_scheduled = true;
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(SAVE_DEBOUNCE).await;
            let Ok((library, file)) = this.update(cx, |this, _| {
                this.save_scheduled = false;
                (this.library.clone(), this.file.clone())
            }) else {
                return;
            };
            let rx = cx.update(|cx| rt::spawn(cx, async move { library.save(&file) }));
            if let Ok(Err(e)) = rx.await {
                let _ = this.update(cx, |this, cx| {
                    this.notice = Some(format!("Couldn't save prompts: {e}"));
                    cx.notify();
                });
            }
        })
        .detach();
    }

    /// A new, empty prompt; returns its id.
    pub fn create(&mut self, cx: &mut Context<Self>) -> String {
        let id = self
            .library
            .add(Prompt::new::<&str>("New prompt", "", &[]), now());
        self.changed(cx);
        id
    }

    pub fn duplicate(&mut self, id: &str, cx: &mut Context<Self>) -> Option<String> {
        let source = self.library.get(id)?.clone();
        let mut copy = Prompt::new(
            &format!("{} (copy)", source.title),
            &source.body,
            &source.tags,
        );
        copy.favorite = false;
        let id = self.library.add(copy, now());
        self.changed(cx);
        Some(id)
    }

    pub fn update(&mut self, id: &str, cx: &mut Context<Self>, edit: impl FnOnce(&mut Prompt)) {
        if self.library.update(id, now(), edit) {
            self.changed(cx);
        }
    }

    pub fn toggle_favorite(&mut self, id: &str, cx: &mut Context<Self>) {
        self.update(id, cx, |p| p.favorite = !p.favorite);
    }

    pub fn remove(&mut self, id: &str, cx: &mut Context<Self>) {
        if self.library.remove(id) {
            cx.emit(PromptStoreEvent::Removed(id.to_string()));
            self.changed(cx);
        }
    }

    pub fn mark_used(&mut self, id: &str, cx: &mut Context<Self>) {
        self.library.mark_used(id, now());
        self.changed(cx);
    }

    /// Imports a WhatsApp chat export file (read off the UI thread).
    pub fn import_whatsapp_file(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        let rx = rt::spawn(cx, async move {
            let bytes = std::fs::read(&path)?;
            Ok::<_, std::io::Error>(import_whatsapp(&String::from_utf8_lossy(&bytes), now()))
        });
        cx.spawn(async move |this, cx| {
            let result = rx.await;
            let _ = this.update(cx, |this, cx| {
                this.notice = Some(match result {
                    Ok(Ok(prompts)) => {
                        let found = prompts.len();
                        let added = this.library.import(prompts);
                        match (found, added) {
                            (0, _) => {
                                "No prompts in that file (short messages and media are skipped)."
                                    .into()
                            }
                            (f, 0) => format!("Found {f} prompts; all were already saved."),
                            (f, a) if a == f => format!("Imported {a} prompts from WhatsApp."),
                            (f, a) => {
                                format!("Imported {a} new prompts ({} already saved).", f - a)
                            }
                        }
                    }
                    Ok(Err(e)) => format!("Couldn't read the file: {e}"),
                    Err(_) => "Import stopped.".into(),
                });
                this.changed(cx);
            });
        })
        .detach();
    }
}
