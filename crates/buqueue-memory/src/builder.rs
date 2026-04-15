//! Builder and backend entry point for the in-memory backend
//!
//! `MemoryBackend` exposes `builder()` as a plain inherent method

use std::{
    collections::{BTreeMap, HashMap},
    sync::Arc,
};

use buqueue_core::{
    backend::BackendBuilder, dlq::DlqConfig, error::BuqueueResult, prelude::ShutdownHandle,
};
use tokio::sync::{Mutex, mpsc};

use crate::{
    MemoryConfig, consumer::MemoryConsumer, envelope::SharedState, producer::MemoryProducer,
};

// ------ MemoryBuilder ------------------

/// Fulent builder for the in-memory backend
///
/// Obtain one via `MemoryBackend::builder`
pub struct MemoryBuilder {
    pub(crate) config: MemoryConfig,
    pub(crate) dlq_config: Option<DlqConfig>,
}

impl BackendBuilder for MemoryBuilder {
    type Producer = MemoryProducer;
    type Consumer = MemoryConsumer;

    fn dead_letter_queue(mut self, config: DlqConfig) -> Self {
        self.dlq_config = Some(config);
        self
    }

    async fn build_pair(self) -> BuqueueResult<(MemoryProducer, MemoryConsumer)> {
        let capacity = self.config.capacity.unwrap_or(1024);
        let (tx, rx) = mpsc::channel(capacity);

        let (dlq_tx, dlq_config) = if let Some(cfg) = self.dlq_config {
            let (dlq_tx, _dlq_rx) = mpsc::channel(capacity);
            (Some(dlq_tx), Some(cfg))
        } else {
            (None, None)
        };

        let shared = Arc::new(Mutex::new(SharedState {
            dlq_config,
            dlq_tx,
            nack_counts: HashMap::new(),
            scheduled: BTreeMap::new(),
        }));

        let (liveness_tx, liveness_rx) = mpsc::channel(1);

        let shutdown = ShutdownHandle::new();
        let producer = MemoryProducer {
            tx: tx.clone(),
            shared: Arc::clone(&shared),
            _liveness: liveness_tx,
        };
        let consumer = MemoryConsumer {
            rx,
            tx,
            shared,
            shutdown,
            liveness_rx,
        };

        Ok((producer, consumer))
    }

    async fn build_producer(self) -> BuqueueResult<MemoryProducer> {
        let (p, _) = self.build_pair().await?;
        Ok(p)
    }

    async fn build_consumer(self) -> BuqueueResult<Self::Consumer> {
        let (_, c) = self.build_pair().await?;
        Ok(c)
    }
}

// ------ MemoryBackend -----------------

/// The in-memory backend
///
/// No broker, no Docker, no network, backed by `tokio::sync::mpsc` channel.
/// Use this in all your unit and integration tests.
///
/// ## Usage
///
/// ```rust
/// # tokio_test::block_on(async {
/// use buqueue_memory::{MemoryBackend, MemoryConfig};
/// use buqueue_core::prelude::*;
///
/// let (producer, mut consumer) = MemoryBackend::builder(MemoryConfig::default())
///     .build_pair()
///     .await
///     .unwrap();
///
/// producer.send(Message::from_json(&42u32).unwrap()).await.unwrap();
/// let delivery = consumer.receive().await.unwrap();
/// assert_eq!(delivery.payload_json::<u32>().unwrap(), 42);
/// delivery.ack().await.unwrap();
/// # });
/// ```
pub struct MemoryBackend;

impl MemoryBackend {
    /// Create a builder for the in-memory backend
    ///
    /// This is a plain inherent method, there is no `QueueBackend` trait
    /// The call site looks the same as every other backend
    ///
    /// ```rust,ignore
    /// MemoryBackend::builder(MemoryConfig::default())
    ///     .dead_letter_queue(dlq) // optional
    ///     .make_dynamic() // optional
    ///     .build_pair()
    ///     .await?
    /// ```
    #[must_use]
    pub fn builder(config: MemoryConfig) -> MemoryBuilder {
        MemoryBuilder {
            config,
            dlq_config: None,
        }
    }
}
