//! Internal state management for the memory backend

use std::collections::{BTreeMap, HashMap};

use buqueue_core::dlq::DlqConfig;
use bytes::Bytes;
use chrono::{DateTime, Utc};
use tokio::sync::mpsc;

/// A message as it travles thourgh the in-memory channel
///
/// Carries the original payload and metadata plus delivery tracking state
#[derive(Debug, Clone)]
pub(crate) struct Envelope {
    /// Unique ID assigned on send, used to track nack counts per message
    pub(crate) id: String,
    #[allow(dead_code)]
    pub(crate) payload: Bytes,
    #[allow(dead_code)]
    pub(crate) headers: HashMap<String, String>,
    #[allow(dead_code)]
    pub(crate) routing_key: Option<String>,
    /// Starts as 1, increment on each nack'd cycle
    pub(crate) delivery_count: u32,
    #[allow(dead_code)]
    pub(crate) first_delivered: DateTime<Utc>,
}

/// Mutable state shared between the producer and the ack handle
///
/// Wrapped in `Arc<Mutex<>>` so both side can access it safely across tasks
#[derive(Debug)]
pub(crate) struct SharedState {
    pub(crate) dlq_config: Option<DlqConfig>,
    pub(crate) dlq_tx: Option<mpsc::Sender<Envelope>>,
    /// How many times each message ID has been nack'd
    pub(crate) nack_counts: HashMap<String, u32>,
    /// Schedules messages not yet due for delivery
    ///
    /// Using `Vec<Envelop>` per key handles the case of
    /// two messages scheduled for the exact same millisecond
    pub(crate) scheduled: BTreeMap<DateTime<Utc>, Vec<Envelope>>,
}

impl SharedState {
    /// Returns the timestamp of the earliest scheduled message, if any
    pub(crate) fn next_scheduled_at(&self) -> Option<DateTime<Utc>> {
        self.scheduled.keys().next().copied()
    }

    /// Drain all scheduled envelopes whose `deliver_at <= now` and send them
    /// into `tx`. Called by the consumer before each receive/`try_receive`
    pub(crate) async fn flush_due_scheduled(&mut self, tx: &mpsc::Sender<Envelope>) {
        let now = Utc::now();

        let remaining = self
            .scheduled
            .split_off(&(now + chrono::Duration::nanoseconds(1)));
        let due = std::mem::replace(&mut self.scheduled, remaining);
        for (_delivery_at, envelopes) in due {
            for envelope in envelopes {
                let _ = tx.send(envelope).await;
            }
        }
    }
}
