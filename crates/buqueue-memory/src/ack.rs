//! Ack/nack handle for in-memory deliveries

use std::{pin::Pin, sync::Arc};

use crate::envelope::{Envelope, SharedState};
use buqueue_core::{delivery::AckHandle, error::BuqueueResult};
use tokio::sync::{Mutex, mpsc};

/// The ack handle attached to every `MemoryConsumer` delivery
///
/// - `ack()`: drops the message (not requeued)
/// - `nack()`: requeues with an incremented delivery count, or routes
///   to the DLQ once `max_receive_count` is reached
#[derive(Debug)]
#[allow(dead_code)]
pub(crate) struct MemoryAckHandle {
    pub(crate) envelope: Envelope,
    pub(crate) requeue_tx: mpsc::Sender<Envelope>,
    pub(crate) shared: Arc<Mutex<SharedState>>,
}

impl AckHandle for MemoryAckHandle {
    fn ack(&self) -> Pin<Box<dyn Future<Output = BuqueueResult<()>> + Send + '_>> {
        // Simply drop, not requeued
        Box::pin(async move { Ok(()) })
    }

    fn nack(&self) -> Pin<Box<dyn Future<Output = BuqueueResult<()>> + Send + '_>> {
        Box::pin(async move {
            let mut state = self.shared.lock().await;

            let max = state
                .dlq_config
                .as_ref()
                .map_or(u32::MAX, |c| c.max_receive_count);

            let count = state
                .nack_counts
                .entry(self.envelope.id.clone())
                .or_insert(0);
            *count += 1;

            if *count >= max {
                // Route the DLQ if configured, otherwise silently drop
                if let Some(dlq_tx) = &state.dlq_tx {
                    let mut dlq_envelope = self.envelope.clone();
                    dlq_envelope.delivery_count += 1;
                    let _ = dlq_tx.send(dlq_envelope).await;
                }
            } else {
                // Requeue with incremented delivery count
                let mut requeued = self.envelope.clone();
                requeued.delivery_count += 1;
                let _ = self.requeue_tx.send(requeued).await;
            }
            Ok(())
        })
    }
}
