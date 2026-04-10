//! Internal state management for the memory backend

use std::collections::HashMap;

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
    pub(crate) header: HashMap<String, String>,
    #[allow(dead_code)]
    pub(crate) routing_key: Option<String>,
    /// Starts as 1, increment on each nack'd cycle
    pub(crate) delivery_count: u32,
    #[allow(dead_code)]
    pub(crate) first_delivered: DateTime<Utc>,
    #[allow(dead_code)]
    /// If `Some`, the consumer will sleep until this time before delivering
    pub(crate) delivery_at: Option<DateTime<Utc>>,
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
}
