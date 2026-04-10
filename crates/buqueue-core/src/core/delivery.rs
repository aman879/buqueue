//! The `Delivery` type: What you receive from a queue
//!
//! ## Rust 2024 edition notes
//!
//! The `AckHandle` trait uses native aync function in traits
//! No `#[async_trait]` proc-macro anywhere
//!
//! ## Lifecycle
//!
//! ```text
//!  consumer.receive()
//!       │
//!       ▼
//!   Delivery ──► .payload_json::<T>()  ──► your Rust type
//!       │
//!       ├──► .ack().await   — success: broker discards the message
//!       └──► .nack().await  — failure: broker redelivers (or DLQ after N attempts)
//! ```
//!
//! Every `Delivery` must eventually be ack'd and nack'd. Dropping withouth
//! calling either causes redelivery after the broker's visibility timeout
//! intentional crash safety, but always call `nack()` explicitly so the
//! delivery count increments correctly

use crate::prelude::{BuqueueError, BuqueueResult, ErrorKind};
use bytes::Bytes;
use chrono::{DateTime, Utc};
use serde::de::DeserializeOwned;
use std::{collections::HashMap, pin::Pin, sync::Arc};

/// A message received from a queue, ready to be processed and acknowledged.
#[derive(Debug)]
pub struct Delivery {
    pub(crate) payload: Bytes,
    pub(crate) headers: HashMap<String, String>,
    pub(crate) routing_key: Option<String>,
    pub(crate) first_delivered_at: Option<DateTime<Utc>>,
    pub(crate) ack_handle: Arc<dyn AckHandle>,
    #[allow(clippy::struct_field_names)]
    pub(crate) delivery_count: u32,
}

impl Delivery {
    /// Construct a `Delivery`. called by backend implementation only
    pub fn new(
        payload: Bytes,
        headers: HashMap<String, String>,
        routing_key: Option<String>,
        delivery_count: u32,
        first_delivered_at: Option<DateTime<Utc>>,
        ack_handle: Arc<dyn AckHandle>,
    ) -> Self {
        Self {
            payload,
            headers,
            routing_key,
            first_delivered_at,
            ack_handle,
            delivery_count,
        }
    }

    /// Return the raw payload bytes
    pub fn payload(&self) -> &Bytes {
        &self.payload
    }

    /// Deserialise the payload as JSON into the type `T`
    ///
    /// # Errors
    ///
    /// Returns `ErrorKind::DeserializationFailed` if the bytes are not valid
    /// JSON or the schema deosn't match. Check `schema-version` header if you
    /// version your events
    pub fn payload_json<T: DeserializeOwned>(&self) -> BuqueueResult<T> {
        serde_json::from_slice(&self.payload).map_err(|e| {
            BuqueueError::with_source(ErrorKind::DeserializationFailed(e.to_string()), e)
        })
    }

    /// Returns the payload as a UTF-8 string slice
    ///
    /// # Errors
    ///
    /// Returns `ErrorKind::DeserializationFailed` if the bytes are not valid
    /// UTF-8.
    pub fn payload_str(&self) -> BuqueueResult<&str> {
        str::from_utf8(&self.payload).map_err(|e| {
            BuqueueError::with_source(ErrorKind::DeserializationFailed(e.to_string()), e)
        })
    }

    /// Returns a single header value by key
    pub fn header(&self, key: &str) -> Option<&str> {
        self.headers.get(key).map(String::as_str)
    }

    /// Returns all headers.
    pub fn headers(&self) -> &HashMap<String, String> {
        &self.headers
    }

    /// Returns the routing key, if one was set on the original message
    pub fn routing_key(&self) -> Option<&str> {
        self.routing_key.as_deref()
    }

    /// Returns the number of times this message has been delivered
    ///
    /// Starts at 1. If > 1, a prevoius processing attemp failed or timed out
    pub fn delivery_count(&self) -> u32 {
        self.delivery_count
    }

    /// Returns the UTC timestamp of the first delivery attempt, if available
    pub fn first_delivery_at(&self) -> Option<DateTime<Utc>> {
        self.first_delivered_at
    }

    /// Return `true` if this is not the first delivery attempt
    ///
    /// Log a warning when this is true, your handler should be idempotent
    pub fn is_redelivery(&self) -> bool {
        self.delivery_count > 1
    }

    /// Acknowledge: tell the broker this message was processed successfully
    ///
    /// # Errors
    ///
    /// Return error from emited by backend
    ///
    /// The broker will not redeliver it
    pub async fn ack(self) -> BuqueueResult<()> {
        self.ack_handle.ack().await
    }

    /// Negatively acknowledge: tell the broker this message was not processed.
    ///
    /// # Errors
    ///
    /// Return error from emited by backend
    ///
    /// The broker will redeliver it, or route it to the DLQ if
    /// `max_receive_count` has been reached.
    pub async fn nack(self) -> BuqueueResult<()> {
        self.ack_handle.nack().await
    }
}

/// Internal trait implemeted by each backend to communicate ack/nack to the broker
///
/// Backend implementors: implement this and wrap it in `Arc<dyn AckHandler>`
/// when construction a `Delivery`. Users never interact with this directly
///
/// Uses native async fn in traits
pub trait AckHandle: Send + Sync + std::fmt::Debug {
    /// Send acknowledgement to the broker
    fn ack(&self) -> Pin<Box<dyn Future<Output = BuqueueResult<()>> + Send + '_>>;

    /// Send negative acknowledgement to the broker
    fn nack(&self) -> Pin<Box<dyn Future<Output = BuqueueResult<()>> + Send + '_>>;
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicBool, Ordering};

    use super::*;

    #[derive(Debug)]
    struct SpyAckHandle {
        acked: AtomicBool,
        nacked: AtomicBool,
    }

    impl SpyAckHandle {
        fn new() -> Arc<Self> {
            Arc::new(Self {
                acked: AtomicBool::new(false),
                nacked: AtomicBool::new(false),
            })
        }
    }

    impl AckHandle for SpyAckHandle {
        fn ack(&self) -> Pin<Box<dyn Future<Output = BuqueueResult<()>> + Send + '_>> {
            self.acked.store(true, Ordering::SeqCst);
            Box::pin(async move { Ok(()) })
        }

        fn nack(&self) -> Pin<Box<dyn Future<Output = BuqueueResult<()>> + Send + '_>> {
            self.nacked.store(true, Ordering::SeqCst);
            Box::pin(async move { Ok(()) })
        }
    }

    fn make_delivery(handle: Arc<dyn AckHandle>) -> Delivery {
        Delivery::new(
            Bytes::from_static(b"{\"id\":1}"),
            HashMap::new(),
            Some("orders.placed".into()),
            1,
            None,
            handle,
        )
    }

    #[tokio::test]
    async fn ack_calls_hanlde() {
        let handle = SpyAckHandle::new();
        let spy = Arc::clone(&handle);
        make_delivery(handle).ack().await.unwrap();
        assert!(spy.acked.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn nack_calls_handle() {
        let handle = SpyAckHandle::new();
        let spy = Arc::clone(&handle);
        make_delivery(handle).nack().await.unwrap();
        assert!(spy.nacked.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn payload_json_decode() {
        #[derive(Debug, serde::Deserialize, PartialEq)]
        struct E {
            id: u32,
        }
        let event: E = make_delivery(SpyAckHandle::new()).payload_json().unwrap();
        assert_eq!(event, E { id: 1 });
    }

    #[test]
    fn is_redelivery_false_on_first() {
        assert!(!make_delivery(SpyAckHandle::new()).is_redelivery());
    }

    #[test]
    fn routing_key_accessible() {
        assert_eq!(
            make_delivery(SpyAckHandle::new()).routing_key(),
            Some("orders.placed")
        );
    }
}
