//! The `QueueProducer` trait: the sending side of a queue.
//!
//! ## Rust 2024 edition notes
//!
//! Uses native async function in traits, no `#[async_trait]` proc-macro

use crate::{error::BuqueueResult, message::Message};
use chrono::{DateTime, Utc};
use std::{future::Future, pin::Pin, sync::Arc};

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
///
/// ## Starting acorss tasks
///
/// `QueueProcuder` is implemented for `Arc<T>` where `T: QueueProducer`, so
/// you can clone `Arc` into each task without wrapping in a `Mutex`:
///
/// ```rust,ignore
/// let producer = Arc::new(SqsProducer::new(config).await?);
/// for _ in 0..8 {
///     let p = Arc::clone(&producer);
///     tokio::spwan(async move{ p.send(msg).await });
/// }
/// ```
pub trait QueueProducer: Send + Sync + Sized {
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
    /// - SQS:          native `DelaySeconds` (< 15 min); holding queue for longer delays
    /// - NATS:         `JetStream` native publish-after timestamp
    /// - `RabbitMQ`:   `rabbitmq-delayed-message-exchange` plugin, no official support
    /// - Redis:        sorted-set staging area with a background forwarder task
    /// - Kafka:        deplay topic with a buqueue-managed forwarder consumer
    ///
    /// **The default implementation ignores `delivery_at` and sends immediately**
    /// It emits a `tracing::warn!` so you know it's not scheduled.
    /// Override this in your backend to provide real scheduling
    fn send_at(
        &self,
        message: Message,
        delivery_at: DateTime<Utc>,
    ) -> impl Future<Output = BuqueueResult<MessageId>> + Send {
        async move {
            tracing::warn!(
                %delivery_at,
                "send_at() called but this backend has not implemented scheduling -\
                sending immediately. Override send_at() in yout QueueProducer impl."
            );
            self.send(message).await
        }
    }

    /// Erase this producer's concrete type, returning a `DynProducer`
    ///
    /// Use this when you need to:
    /// - Store a producer in a struct without a generic paramenter
    /// - Select a backend at runtime and store the result uniformly
    /// - Return a producer from a function without exposing the backend type
    ///
    /// ```rust,ignore
    /// let producer: DynProducer = SqsProducer::new(config).await?.into_dyn();
    /// ```
    fn into_dyn<'a>(self) -> BaseDynProducer<'a>
    where
        Self: 'a,
    {
        BaseDynProducer::new(self)
    }
}

// -- ref_delegate! ------------
//
// Implements QueueProcuder for &T, Box<T>, and Arc<T> where T: QueueProducer.
// This means all three wrapper types forward every method to the inner T
// withouth any manual boilerplate. Add new wrapper types here as needed.
//
// #[deny(unconditional_recursion)] guards against accidentally forwarding
// to self instead of **self, which would loop forever at runtime.

macro_rules! ref_delegate {
    ($ty_param:ident, $ty:ty) => {
        #[deny(unconditional_recursion)]
        impl<$ty_param: QueueProducer> QueueProducer for $ty {
            fn send(
                &self,
                message: Message,
            ) -> impl Future<Output = BuqueueResult<MessageId>> + Send {
                (**self).send(message)
            }

            fn send_batch(
                &self,
                messages: Vec<Message>,
            ) -> impl Future<Output = BuqueueResult<Vec<MessageId>>> + Send {
                (**self).send_batch(messages)
            }

            fn send_at(
                &self,
                message: Message,
                delivery_at: DateTime<Utc>,
            ) -> impl Future<Output = BuqueueResult<MessageId>> + Send {
                (**self).send_at(message, delivery_at)
            }
        }
    };
}

ref_delegate!(T, &T);
ref_delegate!(T, Box<T>);
ref_delegate!(T, Arc<T>);

// --- ErasedQueueProducer -------------
//
// The dyn-compatible bridge between QueueProducer and BaseDynProducer.
//
// QueueProducer uses `impl Fturue` which is not dyn-compatible (opaque
// associated types cannot go in a vtable). ErasedQueueProducer uses
// Pin<Box<dyn Future>> which is dyn compatible, fixed size, known ABI
//
// This trait is create-private. Users never interact with it directly

pub(crate) trait ErasedQueueProducer: Send + Sync {
    fn send<'a>(
        &'a self,
        message: Message,
    ) -> Pin<Box<dyn Future<Output = BuqueueResult<MessageId>> + Send + 'a>>;

    fn send_batch<'a>(
        &'a self,
        messages: Vec<Message>,
    ) -> Pin<Box<dyn Future<Output = BuqueueResult<Vec<MessageId>>> + Send + 'a>>;

    fn send_at<'a>(
        &'a self,
        message: Message,
        delivery_at: DateTime<Utc>,
    ) -> Pin<Box<dyn Future<Output = BuqueueResult<MessageId>> + Send + 'a>>;
}

// -- DynProducerInner -----------
//
// Wraps a concrete QueueProducer and implements ErasedQueueProducer for it
// by boxing the future. This is the only place Box::pin(...) is needed.

struct DynProducerInner<P> {
    inner: P,
}

impl<P: QueueProducer> ErasedQueueProducer for DynProducerInner<P> {
    fn send<'a>(
        &'a self,
        message: Message,
    ) -> Pin<Box<dyn Future<Output = BuqueueResult<MessageId>> + Send + 'a>> {
        Box::pin(self.inner.send(message))
    }

    fn send_batch<'a>(
        &'a self,
        messages: Vec<Message>,
    ) -> Pin<Box<dyn Future<Output = BuqueueResult<Vec<MessageId>>> + Send + 'a>> {
        Box::pin(self.inner.send_batch(messages))
    }

    fn send_at<'a>(
        &'a self,
        message: Message,
        delivery_at: DateTime<Utc>,
    ) -> Pin<Box<dyn Future<Output = BuqueueResult<MessageId>> + Send + 'a>> {
        Box::pin(self.inner.send_at(message, delivery_at))
    }
}

// --- BaseDynProducer / DynProducer ---------

/// A type erased producer. Obtain one by calling `.into_dyn()`(`QueueProducer::into_dyn`)
/// on any concrete producer, or via `BackendBuilder::make_dynamic()`
///
/// `DynProducer` is an alias for `BaseDynProducer<'static>`, which is what
/// you want in almost all cases. Use `BaseDynProducer<'a>` only when the
/// underlyinh producer borrows data with a shorter lifetime
pub struct BaseDynProducer<'a>(Box<dyn ErasedQueueProducer + 'a>);

/// A `'static` type erased producer. The standard to hold a producer
/// without a generic parameter.
///
/// ```rust,ignore
/// struct AppState {
///     producer: DynProducer,
/// }
/// ```
pub type DynProducer = BaseDynProducer<'static>;

impl<'a> BaseDynProducer<'a> {
    fn new(inner: impl QueueProducer + 'a) -> Self {
        Self(Box::new(DynProducerInner { inner }))
    }

    /// Send a single message
    ///
    /// # Errors
    ///
    /// Return Error emitted by backend
    pub async fn send(&self, message: Message) -> BuqueueResult<MessageId> {
        self.0.send(message).await
    }

    /// Send a batch of messages
    ///
    /// # Errors
    ///
    /// Return Error emitted by backend
    pub async fn send_batch(&self, messages: Vec<Message>) -> BuqueueResult<Vec<MessageId>> {
        self.0.send_batch(messages).await
    }

    /// Send a message with a scheduled delivery time
    ///
    /// # Errors
    ///
    /// Return Error emitted by backend
    pub async fn send_at(
        &self,
        message: Message,
        delivery_at: DateTime<Utc>,
    ) -> BuqueueResult<MessageId> {
        self.0.send_at(message, delivery_at).await
    }
}

impl std::fmt::Debug for BaseDynProducer<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DynProducer").finish_non_exhaustive()
    }
}
