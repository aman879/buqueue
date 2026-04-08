//! Graceful shutdown signalling for consumers.
//!
//! A `ShutdownHandle` is returned by `QueueConsumer::shutdown_handle()`.
//! Call `ShutdownHandle::shutdown()` from a SIGTERM handler to signal the 
//! consumer to drain and stop.

use std::sync::Arc;

use tokio::sync::watch;


/// A handle that signal a consumer to shutdown gracefully
///
/// Cloneable, you can send copies to multipler tasks.
/// When any copy calls `shutdown()`[(`Self::shutdown`)], all consumers
/// holding a matching handle will see the shutdown signal.
///
/// ## Usage
///
/// ```rust,ignore
/// let shutdown = consumer.shutdown_handle();
///
/// tokio::spwan(async move {
///     tokio::signal::ctrl_c().await.unwrap();
///     shutdown.shutdown().await;
/// });
/// 
/// while let Some(delivery) = consumer.receive_graceful().await {
///     // process...
/// }
/// ```
#[derive(Clone, Debug)]
pub struct ShutdownHandle {
    inner: Arc<ShutdownInner>,
}

/// Internal shared shutdown signal using a `watch` channel.
///
/// The channel broadcasts a boolean flag:
/// - `false` → system is running
/// - `true`  → shutdown requested
///
/// All `ShutdownHandle` clones share this state, allowing coordinated
/// shutdown across async tasks.
///
/// This is an internal implementation detail.
#[derive(Debug)]
pub struct ShutdownInner {
    sender: watch::Sender<bool>,
    receiver: watch::Receiver<bool>,
}

impl ShutdownHandle {
    /// Create a new , active (not yet shutdown) `ShutdownHandle` pair.
    ///
    /// The returned handle is the signal sender. Pass it (or a clone) to
    /// your shutdown task. The receiver side is embedded inside the handle
    /// via `Arc` so consumer can check `is_shutdown()`.
    #[must_use]
    pub fn new() -> Self {
        let (tx, rx) = watch::channel(false);
        Self { 
            inner:  Arc::new(ShutdownInner { 
                sender: tx, 
                receiver: rx, 
            }),
        }
    }

    /// Creats a no-op handle that is never triggered.
    ///
    /// Used as the deault in `QueueConsumer::shutdown_handle()` for
    /// backends that haven't implemented graceful shutdown yet
    #[must_use]
    pub fn new_noop() -> Self {
        Self::new()
    }

    /// Signal all consumers holding this handle to shutdown.
    ///
    /// After this returns, `is_shutdown()`[(`Self::is_shutdown`)] will return
    /// `true` on all clones of this handle.
    pub fn shutdown(&self) {
        // send(true) will neven fail since we hold both end via Arc
        let _ = self.inner.sender.send(true);
    }

    /// Returns `true` if shutdown has been signaled
    #[must_use]
    pub fn is_shutdown(&self) -> bool {
        *self.inner.receiver.borrow()
    }

    /// Await shutdown, suspends the calling task until `shutdown()`[(`Self::is_shutdown`)]
    /// is called. Useful for wiring into a select loop:
    ///
    /// ```rust,ignore
    /// tokio::select! {
    ///     _ = handle.wait_for_shutdown() => { /* clean up */}
    ///     delivery = consumer.receive() => { /* process */ }
    /// }
    /// ```
    pub async fn wait_for_shutdown(&self) {
        let mut rx = self.inner.receiver.clone();
        // Wait until the value becomes true
        let _ = rx.wait_for(|v| *v).await;
    }
}

impl Default for ShutdownHandle {
    fn default() -> Self {
        Self::new()
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn start_not_shutdown() {
        let handle = ShutdownHandle::new();
        assert!(!handle.is_shutdown());
    }

    #[tokio::test]
    async fn shutdown_is_visible_on_clone() {
        let handle = ShutdownHandle::new();
        let clone = handle.clone();

        handle.shutdown();

        assert!(clone.is_shutdown());
    }

    #[tokio::test]
    async fn wait_for_shutdown_resolves_after_signal() {
        let handle = ShutdownHandle::new();
        let trigger = handle.clone();

        tokio::spawn(async move {
            tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
            trigger.shutdown();
        });

        // This will hang forever if shutdown is never signaled
        tokio::time::timeout(
            tokio::time::Duration::from_millis(500), 
            handle.wait_for_shutdown(),
        )
        .await
        .expect("shutdown should have signaled within 500ms");
    }
}