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
//! #[derive(serde::Serialize, serde::Deserialize)]
//! struct MyEvent { id: u32 }
//!
//! let msg = Message::from_json(&MyEvent { id: 1 }).unwrap();
//!
//! // Full builder: any encoding, full header control
//! let msg = Message::builder()
//!     .payload(Bytes::from_static(b"hello"))
//!     .routing_key("orders.placed")
//!     .header("schema-version", "2")
//!     .dedpulication_id("order-99")
//!     .build();
//! ```

use bytes::Bytes;
use serde::Serialize;
use std::collections::HashMap;

use crate::error::{BuqueueError, BuqueueResult, ErrorKind};

/// A message to be sent into a queue
///
/// Built via `Message::builder()` or convenience contructors
/// `Message::form_json` and `Message::from_json_with_key`
#[derive(Debug, Clone)]
#[allow(dead_code)]
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
    /// - `RabbitMQ`: AMQP headers
    /// - Redis: stream entry fields prefixed with `hdr:`
    pub(crate) headers: HashMap<String, String>,

    /// Optional routing key
    ///
    /// Maps to the backends nattive routing concept:
    /// - Kafka: message key (detemines partition)
    /// - NATS: subject
    /// - SQS: `MessageGroupID` (FIFO quques only; ingnored for standard)
    /// - `RabbitMQ`: AMQP routing key
    /// - Redis: stored as a stream field
    pub(crate) routing_key: Option<String>,

    /// Optional deduplication ID.
    ///
    /// - SQS FIFO: mapped to `MEssageDedpulicationID`
    /// - Kafka: used as the idempotent producer key when idempotent is enabled
    /// - Others: storead as the `bq-dedup-id` header - your consumer can read it and
    ///   `implement` application level deduplication manually
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
    #[must_use]
    pub fn builder() -> MessageBulder {
        MessageBulder::default()
    }

    /// Build a `Message` by serialising `value` as JSON
    ///
    /// Sets the `content-type` header to `application/json` automatically
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - serialization to JSON fails.
    /// - build returns an error
    ///
    /// ```rust
    /// # use buqueue_core::message::Message;
    /// # #[derive(serde::Serialize)]
    /// # struct Order { id: u32 }
    /// let msg = Message::from_json(&Order { id: 1}).unwrap();
    /// assert_eq!(msg.header("content-type"), Some("application/json"));
    /// ```
    pub fn from_json<T: Serialize>(value: &T) -> BuqueueResult<Self> {
        let payload = serde_json::to_vec(value).map_err(|e| {
            BuqueueError::with_source(ErrorKind::SerializationFailed(e.to_string()), e)
        })?;

        Self::builder()
            .payload(Bytes::from(payload))
            .header("content-type", "application/json")
            .build()
    }

    /// Build a `Message` by serialising `value` to JSON, with a routing key
    ///
    /// This is the most common pattern in event-driven systems:
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - serialization to JSON fails.
    /// - build returns an error
    ///
    /// ```rust
    /// # use buqueue_core::message::Message;
    /// # #[derive(serde::Serialize)]
    /// # struct Order { id: u32 }
    /// let msg = Message::from_json_with_key(&Order { id: 1 }, "Orders.Placed").unwrap();
    /// assert_eq!(msg.routing_key(), Some("Orders.Placed"));
    /// ```
    pub fn from_json_with_key<T: Serialize>(
        value: &T,
        routing_key: impl Into<String>,
    ) -> BuqueueResult<Self> {
        let payload = serde_json::to_vec(value).map_err(|e| {
            BuqueueError::with_source(ErrorKind::SerializationFailed(e.to_string()), e)
        })?;

        Self::builder()
            .payload(Bytes::from(payload))
            .routing_key(routing_key)
            .header("content-type", "application/json")
            .build()
    }

    /// Returns the raw payload bytes
    pub fn payload(&self) -> &Bytes {
        &self.payload
    }

    /// Returns the value of a header by key, if present
    pub fn header(&self, key: &str) -> Option<&str> {
        self.headers.get(key).map(String::as_str)
    }

    /// Returns all headers as a referenace to the underlying map
    pub fn headers(&self) -> &HashMap<String, String> {
        &self.headers
    }

    /// Return the routing key, if one was set
    pub fn routing_key(&self) -> Option<&str> {
        self.routing_key.as_deref()
    }

    /// Returns the deduplication Id, if one was set
    pub fn deduplication_id(&self) -> Option<&str> {
        self.deduplication_id.as_deref()
    }

    /// Adds a deduplication ID to this message, consuming and returning it
    ///
    /// # Errors
    ///
    /// Returns an error if the ID is an empty string.
    ///
    /// Userful for chaining after `Message::from_json_with_key`
    ///
    /// ```rust
    /// # use buqueue_core::message::Message;
    /// # #[derive(serde::Serialize)]
    /// # struct Order { id: u32 }
    /// let msg = Message::from_json_with_key(&Order { id: 99 }, "orders.placed")
    ///     .unwrap()
    ///    .with_deduplication_id("order-99");
    /// ```
    pub fn with_deduplication_id(mut self, id: impl Into<String>) -> BuqueueResult<Self> {
        let id = id.into();
        if id.is_empty() {
            return Err(BuqueueError::new(ErrorKind::InvalidConfig(
                "deduplication_id must not be empty".into(),
            )));
        }
        self.deduplication_id = Some(id);
        Ok(self)
    }

    /// Adds or overwirtes a header on this message, consuming and returning it
    ///
    /// # Errors
    ///
    /// Validates the key the same way the builder does — returns an error
    /// if the key is empty or starts with the reserved `bq-` prefix.
    ///
    /// ```rust
    /// # use buqueue_core::message::Message;
    /// # use bytes::Bytes;
    /// let msg = Message::builder()
    ///     .payload(Bytes::from_static(b"hello"))
    ///     .build()
    ///     .unwrap()
    ///     .with_header("retry-count", "3")
    ///     .unwrap();
    /// ```
    pub fn with_header(
        mut self,
        key: impl Into<String>,
        value: impl Into<String>,
    ) -> BuqueueResult<Self> {
        let key = key.into();
        let value = value.into();
        validate_header_key(&key)?;
        validate_header_value(&value, &key)?;
        self.headers.insert(key, value);
        Ok(self)
    }
}

const RESERVED_HEADER_PREFIX: &str = "bq-";

fn validate_header_key(key: &str) -> BuqueueResult<()> {
    if key.is_empty() {
        return Err(BuqueueError::new(ErrorKind::InvalidConfig(
            "header key must not be empty".into(),
        )));
    }

    if key.starts_with(RESERVED_HEADER_PREFIX) {
        return Err(BuqueueError::new(ErrorKind::InvalidConfig(format!(
            "header key {key:?} uses the reserved \"bq-\" prefix -\
            this prefix is used internally by buqueue \
            Choose a different prefix for your own headers."
        ))));
    }

    Ok(())
}

fn validate_header_value(value: &str, key: &str) -> BuqueueResult<()> {
    if value.is_empty() {
        return Err(BuqueueError::new(ErrorKind::InvalidConfig(format!(
            "header value for key {key:?} must not be empty — \
             omit the header entirely if you have no value to set"
        ))));
    }
    Ok(())
}

/// Builder
///
/// Fluent builder for `Message`.
///
/// Obtain one with `Message::builder()`
#[derive(Debug, Default)]
#[allow(dead_code)]
pub struct MessageBulder {
    payload: Option<Bytes>,
    headers: HashMap<String, String>,
    routing_key: Option<String>,
    deduplication_id: Option<String>,
}

impl MessageBulder {
    /// Set the rwa payload bytes
    ///
    /// The encoding is entirely up to you - buqueue never inspects or modifies it
    /// Must not be empty, `build()` will return an error if it is
    #[must_use]
    pub fn payload<B: Into<Bytes>>(mut self, payload: B) -> Self {
        self.payload = Some(payload.into());
        self
    }

    /// Add a header key-value pair
    ///
    /// Calling `.header()` multiple times is fine, header accumulate
    /// Calling it twice with the same key will overwrite the previous value.
    ///
    /// Validation is deferred to `build()`:
    /// - Keys must not be empty
    /// - Keys must not start with the reserved `bq-` prefix
    /// - Values must not be empty
    ///
    /// Notes: `key` and `value` accept any `Into<String>` independently, can mix `&str` and `String`
    #[must_use]
    pub fn header(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.insert(key.into(), value.into());
        self
    }

    /// Set the routing key
    ///
    /// Maps to each backend's native routing primitive:
    /// kafa message key, NATS subject, `RabbitMQ` routing key, SQS `MessageGroupID` (FIFO only)
    #[must_use]
    pub fn routing_key(mut self, key: impl Into<String>) -> Self {
        self.routing_key = Some(key.into());
        self
    }

    /// Set the deduplication ID
    ///
    /// Used for SQS FIFO exactly-once dedupilcation and Kafka idempotent prodicers.
    /// Stored as the `bq-dedup-id` header on backends that dob't have native support
    #[must_use]
    pub fn dedpulication_id(mut self, id: impl Into<String>) -> Self {
        self.deduplication_id = Some(id.into());
        self
    }

    /// Validate and Finalise the builder and return the `Message`
    ///
    /// # Errors
    ///
    /// Returns Error if:
    /// - No payload was set or the payload is empty
    /// - Any header key is empty
    /// - Any header key start with the reserved `bq-` prefix
    /// - Any header value is empty
    /// - The deduplication ID is an empty string
    pub fn build(self) -> BuqueueResult<Message> {
        let payload = match self.payload {
            None => {
                return Err(BuqueueError::new(ErrorKind::InvalidConfig(
                    "payload must be set before calling build() - \
                call .payload(bytes) on the builder"
                        .into(),
                )));
            }
            Some(p) if p.is_empty() => {
                return Err(BuqueueError::new(ErrorKind::InvalidConfig(
                    "payload must not be empty - \
                if you need a zero-byte signal message, use a 1-bytes payload \
                or envode intent in a header instead"
                        .into(),
                )));
            }
            Some(p) => p,
        };

        for (key, value) in &self.headers {
            validate_header_key(key)?;
            validate_header_value(value, key)?;
        }

        if let Some(ref id) = self.deduplication_id
            && id.is_empty()
        {
            return Err(BuqueueError::new(ErrorKind::InvalidConfig(
                "deduplication_id must not be empty".into(),
            )));
        }

        Ok(Message {
            payload,
            headers: self.headers,
            routing_key: self.routing_key,
            deduplication_id: self.deduplication_id,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(serde::Serialize, serde::Deserialize, Debug, PartialEq)]
    struct TestEvent {
        id: u32,
        name: String,
    }

    #[test]
    fn builder_sets_all_fields() {
        let msg = Message::builder()
            .payload(Bytes::from_static(b"hello"))
            .routing_key("order.placed")
            .header("schema-version", "3")
            .header("source-service", "checkout")
            .dedpulication_id("order-99")
            .build()
            .unwrap();

        assert_eq!(msg.payload(), &Bytes::from_static(b"hello"));
        assert_eq!(msg.routing_key(), Some("order.placed"));
        assert_eq!(msg.header("schema-version"), Some("3"));
        assert_eq!(msg.header("source-service"), Some("checkout"));
        assert_eq!(msg.deduplication_id(), Some("order-99"));
    }

    #[test]
    fn from_json_sets_content_type() {
        let msg = Message::from_json(&TestEvent {
            id: 1,
            name: "test".into(),
        })
        .unwrap();
        assert_eq!(msg.header("content-type"), Some("application/json"));
    }

    #[test]
    fn from_json_with_key_sets_routing_key() {
        let msg = Message::from_json_with_key(
            &TestEvent {
                id: 1,
                name: "test".into(),
            },
            "orders.placed",
        )
        .unwrap();

        assert_eq!(msg.routing_key(), Some("orders.placed"));
        assert_eq!(msg.header("content-type"), Some("application/json"));
    }

    #[test]
    fn header_overwrite() {
        let msg = Message::builder()
            .payload(Bytes::from_static(b"x"))
            .header("x-key", "first")
            .header("x-key", "second")
            .build()
            .unwrap();

        assert_eq!(msg.header("x-key"), Some("second"));
    }

    #[test]
    fn mixed_key_value_types_compile() {
        let owned_value = String::from("v1");
        let msg = Message::builder()
            .payload(Bytes::from_static(b"x"))
            .header("x-key", owned_value)
            .build()
            .unwrap();

        assert_eq!(msg.header("x-key"), Some("v1"));
    }

    #[test]
    fn error_on_missing_payload() {
        let err = Message::builder().build().unwrap_err();
        assert!(matches!(err.kind, ErrorKind::InvalidConfig(_)));
        assert!(err.to_string().contains("payload must be set"));
    }

    #[test]
    fn error_on_empty_payload() {
        let err = Message::builder()
            .payload(Bytes::new())
            .build()
            .unwrap_err();
        assert!(matches!(err.kind, ErrorKind::InvalidConfig(_)));
        assert!(err.to_string().contains("payload must not be empty"));
    }

    #[test]
    fn error_on_reserved_bq_prefix() {
        let err = Message::builder()
            .payload(Bytes::from_static(b"x"))
            .header("", "value")
            .build()
            .unwrap_err();

        assert!(matches!(err.kind, ErrorKind::InvalidConfig(_)));
        assert!(err.to_string().contains("header key must not be empty"));
    }

    #[test]
    fn error_on_empty_header_key() {
        let err = Message::builder()
            .payload(Bytes::from_static(b"x"))
            .header("", "value")
            .build()
            .unwrap_err();
        assert!(matches!(err.kind, ErrorKind::InvalidConfig(_)));
        assert!(err.to_string().contains("header key must not be empty"));
    }

    #[test]
    fn error_on_empty_header_value() {
        let err = Message::builder()
            .payload(Bytes::from_static(b"x"))
            .header("x-key", "")
            .build()
            .unwrap_err();
        assert!(matches!(err.kind, ErrorKind::InvalidConfig(_)));
        assert!(err.to_string().contains("must not be empty"));
    }

    #[test]
    fn with_header_rejects_reserved_prefix() {
        let msg = Message::builder()
            .payload(Bytes::from_static(b"x"))
            .build()
            .unwrap();
        let err = msg.with_header("bq-custom", "value").unwrap_err();
        assert!(matches!(err.kind, ErrorKind::InvalidConfig(_)));
    }

    #[test]
    fn error_on_empty_deduplication_id() {
        let err = Message::builder()
            .payload(Bytes::from_static(b"x"))
            .dedpulication_id("")
            .build()
            .unwrap_err();
        assert!(matches!(err.kind, ErrorKind::InvalidConfig(_)));
        assert!(
            err.to_string()
                .contains("deduplication_id must not be empty")
        );
    }

    #[test]
    fn with_deduplication_id_rejects_empty() {
        let msg = Message::builder()
            .payload(Bytes::from_static(b"x"))
            .build()
            .unwrap();
        let err = msg.with_deduplication_id("").unwrap_err();
        assert!(matches!(err.kind, ErrorKind::InvalidConfig(_)));
    }
}
