//! The Tokio runtime as a GPUI global, so views can run I/O off the UI thread.

use std::future::Future;

use futures::channel::oneshot;
use gpui_kit::{App, Global};
use pgdesk_core::rt::Runtime;

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
