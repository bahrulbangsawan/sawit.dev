//! What mesurer's world posts (`EngineEvent::World`), each kind to the file that answers it
//! ([`Lane`]); the foundation's kinds here: `ready`, `log`, `store.*` and `clipboard.text`.

use gpui_kit::*;
use sawit_core::browser::mesurer::{
    ClearWhat, Envelope, FromMesurer, Lane, origin_key, parse_envelope,
};
use serde_json::{Value, json};

use super::super::BrowserView;
use super::super::engine::WorldReply;
use super::{agents, capture, e2e_run, keys};

impl BrowserView {
    /// A message from mesurer's world, as the engine hands it over.
    pub(in crate::browser) fn mesurer_message(
        &mut self,
        body: String,
        reply: WorldReply,
        cx: &mut Context<Self>,
    ) {
        match parse_envelope(&body) {
            Ok(envelope) => self.mesurer_receive(envelope, reply, cx),
            Err(e) => reply.err(e),
        }
    }

    /// Sends `envelope` to the file that answers its kind.
    pub(in crate::browser) fn mesurer_receive(
        &mut self,
        envelope: Envelope,
        reply: WorldReply,
        cx: &mut Context<Self>,
    ) {
        match envelope.message.lane() {
            Lane::Foundation => self.mesurer_own(envelope, reply, cx),
            Lane::Capture => capture::handle(self, envelope.message, reply, cx),
            Lane::Agents => agents::handle(self, envelope.message, reply, cx),
            Lane::Keys => keys::handle(self, envelope.message, reply, cx),
        }
    }

    fn mesurer_own(&mut self, envelope: Envelope, reply: WorldReply, cx: &mut Context<Self>) {
        let Envelope { message, url } = envelope;
        // Filed under the page that posted it: a save that lands after a navigation goes to its
        // own origin's file.
        let page = url.unwrap_or_else(|| self.current_url());
        let origin = origin_key(&page);
        let here = origin == origin_key(&self.current_url());
        match message {
            FromMesurer::Ready {
                version,
                url,
                mount_ms,
            } => {
                eprintln!("sawit: mesurer {version} mounted in {mount_ms} ms ({url})");
                self.mesurer.ready = Some(json!({
                    "version": version,
                    "url": url,
                    "mount_ms": mount_ms,
                }));
            }
            FromMesurer::Log { level, text } => eprintln!("sawit: mesurer {level}: {text}"),
            FromMesurer::StoreSettings(settings) => {
                self.mesurer.store.save_settings(settings, cx);
            }
            FromMesurer::StoreWorkspace(workspace) => {
                let project = self.project.clone();
                let workspace = Some(workspace).filter(Value::is_object);
                (self.mesurer.store).save_workspace(project.as_deref(), &origin, workspace, cx);
                if here {
                    self.mesurer_summary_changed(cx);
                }
            }
            FromMesurer::StoreClear { what } => match what {
                ClearWhat::Settings => self.mesurer.store.clear_settings(cx),
                ClearWhat::Workspace => {
                    let project = self.project.clone();
                    (self.mesurer.store).save_workspace(project.as_deref(), &origin, None, cx);
                    if here {
                        self.mesurer_summary_changed(cx);
                    }
                }
            },
            FromMesurer::ClipboardText { text } => {
                if e2e_run() {
                    self.mesurer.last_clipboard = Some(text);
                } else {
                    cx.write_to_clipboard(ClipboardItem::new_string(text));
                }
            }
            other => {
                reply.err(format!("“{}” isn't the foundation's.", other.kind()));
                return;
            }
        }
        reply.ok_value(&Value::Null);
    }
}
