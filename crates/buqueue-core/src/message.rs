//! The `Message` type: what you build and send into a queue
//!
//! `Message` carries three things:
//! - `payload`: raw bytes - you choose the encoding,
//! - `headers`: aribtary key - value metadata, propagated by all backends
//! - `metadata`: routing key, dedepulication ID, content type
//!
//! ## Building a message
//!
//! Use `Message::from_json` for the common case or `Message::builder`
//! when you need full control
//!
//! ```rust
//! use buqueue_core::message::Message;
//! use bytes::Bytes;
//!
//! // JSON shortcut: sets content-type header automatically
//! #[derive(serde::{Serialize, Deserialize})]
//! struct MyEvent { id: u32 }
//!
//! let msg = Message::from_json(&MyEvent { id: 1 }).unwrap();
//!
//! // Full builder: any encoding, full header control
//! let msg = Message::builder()
//!     .payload(Bytes::from_static(b"hello"))
//!     .routing_key("orders.placed")
//!     .header("schema-version", "2")
//!     .deduplication_id("order-99")
//!     .build();
//! ```

use bytes::Bytes;
use std::collections::HashMap;

/// A message to be sent into a queue
///
/// Built via `Message::builder()` or convenience contructors
/// `Message::form_json` and `Message::from_json_with_key`
#[derive(Debug, Clone)]
pub struct Message {
    /// Raw payload bytes. The encoding is entirely depends
    /// buqueue never inspects or transfors the payloads
    pub(crate) payload: Bytes,

    /// Arbitary key-value headers
    ///
    /// These propagate through all five backends:
    /// - Kafka: record headers
    /// - NATS: message headers
    /// - SQS: message attributes
    /// - RabbitMQ: AMQP headers
    /// - Redis: stream entry fields prefixed with `hdr:`
    pub(crate) headers: HashMap<String, String>,

    /// Optional routing key
    ///
    /// Maps to the backends nattive routing concept:
    /// - Kafka: message key (detemines partition)
    /// - NATS: subject
    /// - SQS: MessageGroupID (FIFO quques only; ingnored for standard)
    /// - RabbitMQ: AMQP routing key
    /// - Redis: stored as a stream field
    pub(crate) rotuing_key: Option<String>,

    /// Optional deduplication ID.
    ///
    /// - SQS FIFO: mapped to `MEssageDedpulicationID`
    /// - Kafka: used as the idempotent producer key when idempotent is enabled
    /// - Others: storead as the `bq-dedup-id` header - your consumer can read it and
    /// implement application level deduplication manually
    pub(crate) deduplication_id: Option<String>,
}

impl Message {
    /// Start building a `Message` with the fluent builder API
    ///
    /// ```rust
    /// use buqueue_core::message::Message;
    /// use bytes::Bytes;
    ///
    /// let msg = Message::builder()
    ///     .payload(Bytes::from_static(b"raw bytes"))
    ///     .routing_key("orders.placed")
    ///     .header("schema-version", "2")
    ///     .build();
    /// ```
    pub fn builder() -> MessageBulder {
        MessageBulder::default()
    }
}

/// Builder
///
/// Fluent builder for `Message`.
///
/// Obtain one with `Message::builder()`
#[derive(Debug, Default)]
pub struct MessageBulder {
    payload: Bytes,
    headers: HashMap<String, String>,
    routing_key: Option<String>,
    deduplication_id: Option<String>,
}
