//! The `QueueProducer` trait: the sending side of a queue.
//! 
//! ## Rust 2024 edition notes
//! 
//! Uses native async function in traits, no `#[async_trait]` proc-macro

use crate::{error::BuqueueResult, message::Message};


/// A unique identifier for a sent message, as assigned by the broker.
/// 
/// The format is backend-specific:
/// - Kafka: `{topic}-{partition}-{offset}`
/// - Nats: `JetStream` sequence number as a string
/// - SQS: the SQS `MessageId` UUID
/// - `RabbitMQ`: monotonically incrementing string per channel
/// - Redis: the XADD entry ID
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct MessageId(pub String);

impl std::fmt::Display for MessageId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<String> for MessageId {
    fn from(value: String) -> Self {
        Self(value)
    }
}

impl From<&str> for MessageId {
    fn from(value: &str) -> Self {
        Self(value.to_string())
    }
}

/// Trait for the sending side of a message queue.
/// 
/// All five buqueue backends implement this trait. Write your busniess logic
/// agains this trait to stay backend-generic.
/// 
/// ## Implementing this trait
/// 
/// ```rust,ignore
/// // No #[async_trait] needed- native async fn in traits, Rust 2024 edition.
/// impl QueueProducer for MyBackendProducer {
///     async fn send(&self, message: Message) -> BuqueueResult<MessageId> {
///         // ... publish to your broker ...
///     }
/// }
/// ```
pub trait QueueProducer: Send + Sync {
    /// Send a single message and return its broke-assigned ID.
    /// 
    /// This is the fundamental operation that every backend must implement.
    fn send(&self, message: Message) -> impl Future<Output = BuqueueResult<MessageId>> + Send;

    /// Send multiple messages in one broker operation where supported
    ///
    /// Returns one `MessageID` per message in the same order as input
    /// 
    /// The default sends messages sequentially. Override in your backed
    /// to use native batch APIs
    /// 
    /// If any individual send fails, the error is returned immediately and subsequent
    /// messages in the batch are not sent
    fn send_batch(
        &self,
        messages: Vec<Message>,
    ) -> impl Future<Output = BuqueueResult<Vec<MessageId>>> + Send {
        async move {
            let mut ids = Vec::with_capacity(messages.len());
            for msg in messages {
                ids.push(self.send(msg).await?);
            }
            Ok(ids)
        }
    }

    /// Send a message that will not be visible to consmers until `delivery_at`.
    /// 
    /// The message is accepted by the broker immediately but held back until
    /// the specified UTC timestamp. Your consumer code is unchanged, it simply
    /// receives the message when it becomes visible, with no knowledge it was scheduled.
    /// 
    /// Each backend implemets this differently under the hood:
    /// - SQS: native `DelaySeconds` (< 15 min); holding queue for longer delays
    /// - NATS: `JetStream` native publish-after timestamp
    /// - `RabbitMQ`: 
}