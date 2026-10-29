//! The Tokio runtime as a GPUI global, so views can run I/O off the UI thread.

use std::future::Future;

use futures::channel::oneshot;
use gpui_kit::{App, Global};
use sawit_core::rt::Runtime;

pub struct Rt(pub Runtime);

impl Global for Rt {}

/// Runs `fut` on the Tokio runtime; await the receiver inside `cx.spawn`.
pub fn spawn<F, T>(cx: &App, fut: F) -> oneshot::Receiver<T>
where
    F: Future<Output = T> + Send + 'static,
    T: Send + 'static,
{
    cx.global::<Rt>().0.spawn(fut)
}

/// Runs blocking or CPU-heavy `f` off the UI thread and off the async workers.
pub fn spawn_blocking<F, T>(cx: &App, f: F) -> oneshot::Receiver<T>
where
    F: FnOnce() -> T + Send + 'static,
    T: Send + 'static,
{
    cx.global::<Rt>().0.spawn_blocking(f)
}
