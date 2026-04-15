//! In-memory producer implementation

use std::sync::Arc;

use buqueue_core::{
    error::{BuqueueError, BuqueueResult, ErrorKind},
    prelude::{Message, MessageId, QueueProducer},
};
use chrono::Utc;
use tokio::sync::{Mutex, mpsc};
use uuid::Uuid;

use crate::envelope::{Envelope, SharedState};

/// In-memory producer
///
/// Cheaply cloneable, share across tasks freely with `.clone()` or `Arc`
/// Both the clone and the original write to the same underlying channel
#[derive(Debug, Clone)]
pub struct MemoryProducer {
    pub(crate) tx: mpsc::Sender<Envelope>,
    #[allow(dead_code)]
    pub(crate) shared: Arc<Mutex<SharedState>>,
    pub(crate) _liveness: mpsc::Sender<()>,
}

impl QueueProducer for MemoryProducer {
    async fn send(&self, message: Message) -> BuqueueResult<MessageId> {
        let id = Uuid::new_v4().to_string();
        let envelope = Envelope {
            id: id.clone(),
            payload: message.payload().clone(),
            headers: message.headers().clone(),
            routing_key: message.routing_key().map(str::to_string),
            delivery_count: 1,
            first_delivered: Utc::now(),
        };

        self.tx
            .send(envelope)
            .await
            .map_err(|_| BuqueueError::new(ErrorKind::ConnectionLost))?;

        Ok(MessageId::from(id))
    }

    async fn send_at(
        &self,
        message: Message,
        delivery_at: chrono::DateTime<Utc>,
    ) -> BuqueueResult<MessageId> {
        let id = Uuid::new_v4().to_string();
        let envelope = Envelope {
            id: id.clone(),
            payload: message.payload().clone(),
            headers: message.headers().clone(),
            routing_key: message.routing_key().map(str::to_string),
            delivery_count: 1,
            first_delivered: Utc::now(),
        };

        // Put inyo the scheduled map, Not the main channel
        // The consumer drains due messages before each receive/try_receive,
        // so the message will not be visible until deliver_at has passed.
        {
            let mut state = self.shared.lock().await;
            state
                .scheduled
                .entry(delivery_at)
                .or_insert_with(Vec::new)
                .push(envelope);
        }

        Ok(MessageId::from(id))
    }
}
