//! The `sawit-mesurer` content world (`WKContentWorld worldWithName:`), where mesurer and Sawit's
//! glue run apart from the page's own scripts: what the app evaluates and calls there
//! ([`Engine::world_eval`], [`Engine::world_call`]), and the `sawitMesurer` message handler the glue
//! posts to, which exists in that world alone. It takes messages from the page's main frame only
//! and hands each to the view as [`EngineEvent::World`], with the [`WorldReply`] that answers it.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;

use block2::{DynBlock, RcBlock};
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObject, ProtocolObject};
use objc2::{DefinedClass as _, MainThreadMarker, MainThreadOnly, define_class, msg_send};
use objc2_foundation::{NSDictionary, NSError, NSObjectProtocol, NSString};
use objc2_web_kit::{
    WKContentWorld, WKScriptMessage, WKScriptMessageHandlerWithReply, WKUserContentController,
};
use sawit_core::browser::mesurer::{HANDLER, WORLD};
use wry::WebViewExtMacOS as _;

use super::{Engine, EngineEvent, exception_message};

type ReplyBlock = RcBlock<dyn Fn(*mut AnyObject, *mut NSString)>;

thread_local! {
    /// The reply handlers of the messages not answered yet, by id: WebKit's blocks live on the
    /// main thread, where they are called.
    static REPLIES: RefCell<HashMap<u64, ReplyBlock>> = RefCell::new(HashMap::new());
    static NEXT_REPLY: Cell<u64> = const { Cell::new(1) };
}

/// The answer to one message from mesurer's world: the glue's `bridge()` promise resolves with
/// [`ok`](Self::ok)'s JSON or rejects with [`err`](Self::err)'s message. Answer it on the main
/// thread (the view's tasks run there); one dropped unanswered answers with an error.
pub struct WorldReply(Option<Target>);

enum Target {
    Block(u64),
    #[cfg(test)]
    Channel(futures::channel::oneshot::Sender<Result<String, String>>),
}

impl WorldReply {
    fn block(block: ReplyBlock) -> WorldReply {
        let id = NEXT_REPLY.with(|next| {
            let id = next.get();
            next.set(id + 1);
            id
        });
        REPLIES.with(|replies| replies.borrow_mut().insert(id, block));
        WorldReply(Some(Target::Block(id)))
    }

    /// A reply whose answer the test reads from the receiver.
    #[cfg(test)]
    pub fn channel() -> (
        WorldReply,
        futures::channel::oneshot::Receiver<Result<String, String>>,
    ) {
        let (tx, rx) = futures::channel::oneshot::channel();
        (WorldReply(Some(Target::Channel(tx))), rx)
    }

    /// Answers with `json`, a JSON text.
    pub fn ok(mut self, json: String) {
        self.send(Ok(json));
    }

    /// Answers with `value`.
    pub fn ok_value(self, value: &serde_json::Value) {
        self.ok(value.to_string());
    }

    /// Rejects with `message`.
    pub fn err(mut self, message: impl Into<String>) {
        self.send(Err(message.into()));
    }

    fn send(&mut self, result: Result<String, String>) {
        match self.0.take() {
            Some(Target::Block(id)) => {
                let block = REPLIES
                    .try_with(|replies| replies.borrow_mut().remove(&id))
                    .ok()
                    .flatten();
                let Some(block) = block else { return };
                match result {
                    Ok(json) => {
                        let text = NSString::from_str(&json);
                        let value = Retained::as_ptr(&text).cast::<AnyObject>().cast_mut();
                        block.call((value, std::ptr::null_mut()));
                    }
                    Err(e) => {
                        let text = NSString::from_str(&e);
                        block.call((std::ptr::null_mut(), Retained::as_ptr(&text).cast_mut()));
                    }
                }
            }
            #[cfg(test)]
            Some(Target::Channel(tx)) => {
                let _ = tx.send(result);
            }
            None => {}
        }
    }
}

impl Drop for WorldReply {
    fn drop(&mut self) {
        if self.0.is_some() {
            self.send(Err("Sawit didn't answer mesurer.".into()));
        }
    }
}

struct HandlerIvars {
    on_event: Box<dyn Fn(EngineEvent)>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[ivars = HandlerIvars]
    struct WorldHandler;

    unsafe impl NSObjectProtocol for WorldHandler {}

    unsafe impl WKScriptMessageHandlerWithReply for WorldHandler {
        #[unsafe(method(userContentController:didReceiveScriptMessage:replyHandler:))]
        fn did_receive(
            &self,
            _controller: &WKUserContentController,
            message: &WKScriptMessage,
            reply_handler: &DynBlock<dyn Fn(*mut AnyObject, *mut NSString)>,
        ) {
            let reply = WorldReply::block(reply_handler.copy());
            // A frame in the page (an ad, a preview) can't speak for mesurer.
            if !unsafe { message.frameInfo().isMainFrame() } {
                reply.err("Sawit takes mesurer's messages from the page's main frame only.");
                return;
            }
            let body = unsafe { message.body() }.downcast::<NSString>().ok();
            match body {
                Some(body) => (self.ivars().on_event)(EngineEvent::World {
                    body: body.to_string(),
                    reply,
                }),
                None => reply.err("mesurer's messages are JSON text."),
            }
        }
    }
);

/// The world of a web view, and its handler, registered on the web view's content controller
/// until the engine drops.
pub(super) struct World {
    world: Retained<WKContentWorld>,
    controller: Retained<objc2_web_kit::WKUserContentController>,
    _handler: Retained<WorldHandler>,
}

impl World {
    pub(super) fn register(
        webview: &wry::WebView,
        on_event: Box<dyn Fn(EngineEvent)>,
    ) -> Result<World, String> {
        let mtm = MainThreadMarker::new().ok_or("The browser is made on the main thread.")?;
        let wk = webview.webview();
        let controller = unsafe { wk.configuration().userContentController() };
        let world = unsafe { WKContentWorld::worldWithName(&NSString::from_str(WORLD), mtm) };
        let handler = WorldHandler::alloc(mtm).set_ivars(HandlerIvars { on_event });
        let handler: Retained<WorldHandler> = unsafe { msg_send![super(handler), init] };
        unsafe {
            controller.addScriptMessageHandlerWithReply_contentWorld_name(
                ProtocolObject::from_ref(&*handler),
                &world,
                &NSString::from_str(HANDLER),
            );
        }
        Ok(World {
            world,
            controller,
            _handler: handler,
        })
    }
}

impl Drop for World {
    fn drop(&mut self) {
        unsafe {
            self.controller
                .removeScriptMessageHandlerForName_contentWorld(
                    &NSString::from_str(HANDLER),
                    &self.world,
                );
        }
    }
}

/// A completion handler that hands `callback` the script's value when it is a string (`None` for
/// any other), or the exception WebKit reports.
fn text_handler(
    callback: impl FnOnce(Result<Option<String>, String>) + 'static,
) -> RcBlock<dyn Fn(*mut AnyObject, *mut NSError)> {
    let callback = Cell::new(Some(callback));
    RcBlock::new(move |value: *mut AnyObject, error: *mut NSError| {
        let Some(callback) = callback.take() else {
            return;
        };
        if let Some(error) = unsafe { error.as_ref() } {
            callback(Err(exception_message(error)));
            return;
        }
        let text = unsafe { value.as_ref() }
            .and_then(|v| v.downcast_ref::<NSString>())
            .map(|s| s.to_string());
        callback(Ok(text));
    })
}

impl Engine {
    /// Evaluates `script` in mesurer's world, in the page's main frame; `callback` gets its value
    /// when that is a string, or the exception.
    pub fn world_eval(
        &self,
        script: &str,
        callback: impl FnOnce(Result<Option<String>, String>) + 'static,
    ) {
        let handler = text_handler(callback);
        let wk = self.webview.webview();
        unsafe {
            wk.evaluateJavaScript_inFrame_inContentWorld_completionHandler(
                &NSString::from_str(script),
                None,
                &self.world.world,
                Some(&handler),
            )
        };
    }

    /// Runs `body` as an async function in mesurer's world, with `json` (a JSON text) as its
    /// argument `json`; `callback` gets what it returns (a JSON string), or the exception.
    pub fn world_call(
        &self,
        body: &str,
        json: &str,
        callback: impl FnOnce(Result<Option<String>, String>) + 'static,
    ) {
        let handler = text_handler(callback);
        let key = NSString::from_str("json");
        let value = NSString::from_str(json);
        let args: Retained<NSDictionary<NSString, AnyObject>> =
            NSDictionary::from_slices(&[&*key], &[value.as_ref()]);
        let wk = self.webview.webview();
        unsafe {
            wk.callAsyncJavaScript_arguments_inFrame_inContentWorld_completionHandler(
                &NSString::from_str(body),
                Some(&args),
                None,
                &self.world.world,
                Some(&handler),
            )
        };
    }
}
