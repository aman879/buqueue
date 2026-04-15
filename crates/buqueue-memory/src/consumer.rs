//! In-memory consumer implementation

use std::sync::Arc;

use buqueue_core::{
    error::{BuqueueError, BuqueueResult, ErrorKind},
    prelude::{Delivery, QueueConsumer, ShutdownHandle},
};
use tokio::sync::{Mutex, mpsc};

use crate::{
    ack::MemoryAckHandle,
    envelope::{Envelope, SharedState},
};

/// In-memory consumer
///
/// Receives messages from the channel written to by `MemoryProducer`
/// Not `Clone`, only one consumer per channel
pub struct MemoryConsumer {
    pub(crate) rx: mpsc::Receiver<Envelope>,
    pub(crate) tx: mpsc::Sender<Envelope>,
    pub(crate) shared: Arc<Mutex<SharedState>>,
    pub(crate) shutdown: ShutdownHandle,
    pub(crate) liveness_rx: mpsc::Receiver<()>,
}

impl std::fmt::Debug for MemoryConsumer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MemoryConsumer").finish_non_exhaustive()
    }
}

impl MemoryConsumer {
    pub(crate) fn make_delivery(&self, envelope: Envelope) -> Delivery {
        let handle = Arc::new(MemoryAckHandle {
            envelope: envelope.clone(),
            requeue_tx: self.tx.clone(),
            shared: Arc::clone(&self.shared),
        });
        Delivery::new(
            envelope.payload,
            envelope.headers,
            envelope.routing_key,
            envelope.delivery_count,
            Some(envelope.first_delivered),
            handle,
        )
    }

    /// Flush any schedlued messages that are now due into the main channel
    ///
    /// Must be called before receive/`try_receive` so that scheduled
    /// messages become visible at the correct time
    async fn flush_scheduled(&mut self) {
        let mut state = self.shared.lock().await;
        state.flush_due_scheduled(&self.tx).await;
    }
}

impl QueueConsumer for MemoryConsumer {
    async fn receive(&mut self) -> BuqueueResult<Delivery> {
        if self.shutdown.is_shutdown() {
            return Err(BuqueueError::new(ErrorKind::ConsumerShutdown));
        }

        // deal with now-due scheduled message into the channel before polling
        self.flush_scheduled().await;

        tokio::select! {
            biased;
            envelope = self.rx.recv() => {
                let envelope = envelope.ok_or_else(|| BuqueueError::new(ErrorKind::ConnectionLost))?;
                Ok(self.make_delivery(envelope))
            }
            _ = self.liveness_rx.recv() => {
                Err(BuqueueError::new(ErrorKind::ConnectionLost))
            }
        }
    }

    async fn try_receive(&mut self) -> BuqueueResult<Option<Delivery>> {
        // FLlush scheduled messages first, a scheduled message whose time
        // has arrived must be visible to try_receive, not silently skipped
        self.flush_scheduled().await;
        match self.rx.try_recv() {
            Ok(envelope) => Ok(Some(self.make_delivery(envelope))),
            Err(mpsc::error::TryRecvError::Empty) => Ok(None),
            Err(mpsc::error::TryRecvError::Disconnected) => {
                Err(BuqueueError::new(ErrorKind::ConnectionLost))
            }
        }
    }

    fn shutdown_handle(&self) -> ShutdownHandle {
        self.shutdown.clone()
    }

    // Overrides the default, races receive() agains shutdown signal directly
    // using `biased` so shutdown always takes priority
    async fn receive_graceful(&mut self) -> Option<BuqueueResult<Delivery>> {
        if self.shutdown.is_shutdown() {
            return None;
        }

        let shutdown = self.shutdown.clone();
        tokio::select! {
            biased;
            () = shutdown.wait_for_shutdown() => None,
            delivery = self.receive() => Some(delivery)
        }
    }
}
