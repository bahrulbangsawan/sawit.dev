//! A Tokio runtime living on its own thread, so UI code never blocks on I/O.
//!
//! Results come back over `futures` channels, which any executor (including
//! GPUI's) can await.

use futures::channel::{mpsc, oneshot};
use futures::future::BoxFuture;
use std::future::Future;
use tokio::runtime::Handle;

#[derive(Clone)]
pub struct Runtime {
    handle: Handle,
}

impl Runtime {
    /// Starts a multi-thread Tokio runtime owned by a dedicated, parked thread.
    pub fn start() -> Runtime {
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::Builder::new()
            .name("sawit-tokio".into())
            .spawn(move || {
                let runtime = tokio::runtime::Builder::new_multi_thread()
                    .worker_threads(2)
                    .thread_name("sawit-tokio-worker")
                    .enable_all()
                    .build()
                    .expect("failed to build tokio runtime");
                tx.send(runtime.handle().clone())
                    .expect("runtime receiver dropped");
                runtime.block_on(std::future::pending::<()>());
            })
            .expect("failed to spawn tokio thread");
        Runtime {
            handle: rx.recv().expect("tokio thread died during startup"),
        }
    }

    /// Runs `fut` on the Tokio runtime; the receiver resolves with its output.
    pub fn spawn<F, T>(&self, fut: F) -> oneshot::Receiver<T>
    where
        F: Future<Output = T> + Send + 'static,
        T: Send + 'static,
    {
        let (tx, rx) = oneshot::channel();
        self.handle.spawn(async move {
            let _ = tx.send(fut.await);
        });
        rx
    }

    /// Runs a producer on the Tokio runtime that streams items to the receiver.
    pub fn spawn_stream<T: Send + 'static>(
        &self,
        f: impl FnOnce(mpsc::UnboundedSender<T>) -> BoxFuture<'static, ()> + Send + 'static,
    ) -> mpsc::UnboundedReceiver<T> {
        let (tx, rx) = mpsc::unbounded();
        self.handle.spawn(async move { f(tx).await });
        rx
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::StreamExt;
    use std::time::Duration;

    #[test]
    fn spawn_returns_value_without_blocking_caller_thread() {
        let rt = Runtime::start();
        let rx = rt.spawn(async {
            tokio::time::sleep(Duration::from_millis(50)).await;
            42
        });
        assert_eq!(futures::executor::block_on(rx).unwrap(), 42);
    }

    #[test]
    fn spawn_stream_delivers_items_in_order() {
        let rt = Runtime::start();
        let rx = rt.spawn_stream(|tx| {
            Box::pin(async move {
                for i in 0..3 {
                    tx.unbounded_send(i).unwrap();
                }
            })
        });
        assert_eq!(
            futures::executor::block_on(rx.collect::<Vec<_>>()),
            vec![0, 1, 2]
        );
    }
}
