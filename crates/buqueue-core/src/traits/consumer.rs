//! The `QueueConsumer` trait and its dynamic wrapper `DynConsumer`
//!
//! ## Architecture
//!
//! Mirrors the layers exactly:
//!
//! ```text
//! QueueConsumer          — generic trait, impl Future, Send (not Sized — needs &mut self)
//!       │  .into_dyn()
//!       ▼
//! ErasedQueueConsumer    — dyn-compatible bridge (Pin<Box<dyn Future>>), crate-private
//!       │
//!       ▼
//! BaseDynConsumer        — the type users actually hold when they want type erasure
//! (= DynConsumer)
//! ```
//!
//! ## Why `QueueConsumer` is not Sized
//!
//! `QueueProducer` is Sized because producer only take `&self`. So, you can
//! put them behind `Arc`. `QueueConsumer` takes `&mut self` (receive mutates
//! internal state like a channel cursor), so it cannot be `Sized + shared`.
//! The `ref_delegate!` macro therefore can only covers `Box<T>` and `&mut T`,
//! not `Arc<T>`.

use std::pin::Pin;

use futures::{Stream, stream};

use crate::prelude::{BuqueueResult, Delivery, ShutdownHandle};

// -------- QueueConsumer ------------------------------

/// Traits for receiving side of a message queue
///
/// Implement this for your backend. Use `DynConsumer (via .into_dyn())`
/// when you need type erasure
///
/// ## Basic usage
///
/// ```rust,ignore
/// while let Some(delivery) = consumer.receive_graceful().await {
///     let event: MyEvent = delivery.payload_json()?;
///     process(event).await?;
///     delivery.ack().await?;
/// }
/// ```
///
/// ## Implementing this trait
///
/// Only `receive()`[`Self::receive`] is required. All other methods have correct
/// default implementations.
///
/// ```rust,ignore
/// impl QueueConsumer for MyBackendConsumer {
///     async fn receive(&mut self) -> BuqueueResult<Delivery> {
///        // ... poll your broker ...
///     }
/// }
/// ```
pub trait QueueConsumer: Send {
    /// Block until a message is available and return it.
    ///
    /// The only method you must implement. The returned `Delivery` must
    /// eventually the ack'd and nack'd
    fn receive(&mut self) -> impl Future<Output = BuqueueResult<Delivery>> + Send;

    /// Returns a message immediately if one is availabe, or `None` if empty.
    ///
    /// Does not block. Useful in tests to assert a queue is drained.
    /// The default wraps `receive()` in a zero-duration timeout.
    /// Override in your backend for a true non-blocking poll.
    fn try_receive(&mut self) -> impl Future<Output = BuqueueResult<Option<Delivery>>> + Send {
        async move {
            use tokio::time::{Duration, timeout};
            match timeout(Duration::ZERO, self.receive()).await {
                Ok(result) => result.map(Some),
                Err(_) => Ok(None),
            }
        }
    }

    /// Request up to `max` messages in one call.
    ///
    /// Always blocks for the first message, the poll non-blocking until
    /// `max` is reached or the queue is drained. Never returns an empty vec.
    ///
    /// Override in your backend to use Native batch APIs
    /// (SQS `ReceiveMessage`, Kafka `poll`, NATS `fetch`, etc.).
    fn receive_batch(
        &mut self,
        max: usize,
    ) -> impl Future<Output = BuqueueResult<Vec<Delivery>>> + Send {
        async move {
            let first = self.receive().await?;
            let mut deliveries = vec![first];
            while deliveries.len() < max {
                match self.try_receive().await? {
                    Some(delivery) => deliveries.push(delivery),
                    None => break,
                }
            }
            Ok(deliveries)
        }
    }

    /// Returns a `ShutdownHandle` to signal this consumer to shutdown gracefully
    ///
    /// The default returns a no-noop handle. Override in your backend to wire
    /// up real shutdown signalling
    fn shutdown_handle(&self) -> ShutdownHandle {
        ShutdownHandle::new_noop()
    }

    /// Like `receive()`[(`Self::receive`)], but returns `None` after shutdown
    ///
    /// Races between a new message arriving and shutdown sginal,
    /// whicever resolves first wins:
    ///
    /// ```rust,ignore
    /// while let Some(delivery) = consumer.receive_graceful().await {
    ///     delivery.payload_json::<MyEvent>()?.process().await?;
    ///     delivery.ack().await?;
    /// }
    /// ```
    fn receive_graceful(&mut self) -> impl Future<Output = Option<BuqueueResult<Delivery>>> + Send {
        async move {
            if self.shutdown_handle().is_shutdown() {
                return None;
            }
            let shutdown = self.shutdown_handle();
            tokio::select! {
                delivery = self.receive() => Some(delivery),
                () = shutdown.wait_for_shutdown() => None,
            }
        }
    }

    /// Convert this consumer into an async `Stream` of deliverie
    ///
    /// Consumes `self`. Implemented with `futures::stream::unfold`, no unsafe
    ///
    /// ```rust,ignore
    /// use futures::StreamExt;
    /// consumer.into_stream()
    ///    .take(100)
    ///     .for_each_concurrent(8, |res| async move {
    ///         res.unwrap().ack().await.unwrap();
    ///     })
    ///     .await;
    /// ```
    fn into_stream(self) -> impl Stream<Item = BuqueueResult<Delivery>> + Send
    where
        Self: Sized + 'static,
    {
        stream::unfold((self, false), |(mut consumer, errored)| async move {
            if errored {
                return None;
            }
            match consumer.receive_graceful().await {
                Some(Ok(delivery)) => Some((Ok(delivery), (consumer, false))),
                Some(Err(e)) => Some((Err(e), (consumer, true))),
                None => None,
            }
        })
    }

    /// Erase this consumer's concrete type, returning a `DynConsumer`
    ///
    /// Use this when you need to:
    /// - Store a consumer in a struct without a generic parameter
    /// - Select a backend at runtime and store the result uniformly
    /// - Return a consumer from a function without exposing the backend type
    ///
    /// ```rust,ignore
    /// let consumer: DynConsumer = SqsConsumer::new(config).await?.into_dyn();
    /// ```
    fn into_dyn<'a>(self) -> BaseDynConsumer<'a>
    where
        Self: Sized + 'a,
    {
        BaseDynConsumer::new(self)
    }
}

// ------- ref_delegate! ------------------------
//
// Implements QueueConsumer for &mut T and Box<T> where T: QueueConsumer
// Arc<T> is excluded, Consumer take &mut self so shared ownership via Arc
// is not possible without a Mutex, which is the caller's responsibility

macro_rules! ref_delefate {
    ($ty_params:ident, $ty:ty) => {
        #[deny(unconditional_recursion)]
        impl<$ty_params: QueueConsumer> QueueConsumer for $ty {
            fn receive(&mut self) -> impl Future<Output = BuqueueResult<Delivery>> + Send {
                (**self).receive()
            }

            fn try_receive(
                &mut self,
            ) -> impl Future<Output = BuqueueResult<Option<Delivery>>> + Send {
                (**self).try_receive()
            }

            fn receive_batch(
                &mut self,
                max: usize,
            ) -> impl Future<Output = BuqueueResult<Vec<Delivery>>> + Send {
                (**self).receive_batch(max)
            }

            fn shutdown_handle(&self) -> ShutdownHandle {
                (**self).shutdown_handle()
            }

            fn receive_graceful(
                &mut self,
            ) -> impl Future<Output = Option<BuqueueResult<Delivery>>> + Send {
                (**self).receive_graceful()
            }
        }
    };
}

ref_delefate!(T, &mut T);
ref_delefate!(T, Box<T>);

// ------ ErasedQueueConsumer ------------------
//
// The dyn-compatible bridge. Same reasoning as ErasedQueueProducer.
// into_stream and into_dyn are excluded, both require Self: Sized

pub(crate) trait ErasedQueueConsumer: Send {
    fn receive<'a>(
        &'a mut self,
    ) -> Pin<Box<dyn Future<Output = BuqueueResult<Delivery>> + Send + 'a>>;

    fn try_receive<'a>(
        &'a mut self,
    ) -> Pin<Box<dyn Future<Output = BuqueueResult<Option<Delivery>>> + Send + 'a>>;

    fn receive_batch<'a>(
        &'a mut self,
        max: usize,
    ) -> Pin<Box<dyn Future<Output = BuqueueResult<Vec<Delivery>>> + Send + 'a>>;

    fn shutdown_handle(&self) -> ShutdownHandle;

    fn receive_graceful<'a>(
        &'a mut self,
    ) -> Pin<Box<dyn Future<Output = Option<BuqueueResult<Delivery>>> + Send + 'a>>;
}

// --- DynConsumerInner ------------------

struct DynConsumerInner<C> {
    inner: C,
}

impl<C: QueueConsumer> ErasedQueueConsumer for DynConsumerInner<C> {
    fn receive<'a>(
        &'a mut self,
    ) -> Pin<Box<dyn Future<Output = BuqueueResult<Delivery>> + Send + 'a>> {
        Box::pin(self.inner.receive())
    }

    fn try_receive<'a>(
        &'a mut self,
    ) -> Pin<Box<dyn Future<Output = BuqueueResult<Option<Delivery>>> + Send + 'a>> {
        Box::pin(self.inner.try_receive())
    }

    fn receive_batch<'a>(
        &'a mut self,
        max: usize,
    ) -> Pin<Box<dyn Future<Output = BuqueueResult<Vec<Delivery>>> + Send + 'a>> {
        Box::pin(self.inner.receive_batch(max))
    }

    fn shutdown_handle(&self) -> ShutdownHandle {
        self.inner.shutdown_handle()
    }

    fn receive_graceful<'a>(
        &'a mut self,
    ) -> Pin<Box<dyn Future<Output = Option<BuqueueResult<Delivery>>> + Send + 'a>> {
        Box::pin(self.inner.receive_graceful())
    }
}

// ---- BaseDynConsumer / DynConsumer ----------------------

/// A type erased consumer. Obtain one by calling `.into_dyn()`[(`QueueConsumer::into_dyn`)]
/// on any concrete consumer, or via `BackendBuilder::make_dynamic()`
pub struct BaseDynConsumer<'a>(Box<dyn ErasedQueueConsumer + 'a>);

/// A `'static` type-erased consumer. The standard way to hold a consumer
/// without a generic parameter.
///
/// ```rust,ignore
/// struct AppState {
///    consumer: DynConsumer,
/// }
/// ```
pub type DynConsumer = BaseDynConsumer<'static>;

impl<'a> BaseDynConsumer<'a> {
    fn new(inner: impl QueueConsumer + 'a) -> Self {
        Self(Box::new(DynConsumerInner { inner }))
    }

    /// Block until a message is available.
    ///
    /// # Errors
    ///
    /// Return Error emitted by backend
    pub async fn receive(&mut self) -> BuqueueResult<Delivery> {
        self.0.receive().await
    }

    /// Non-blocking poll, returns `None` if queue is currently empty
    ///
    /// # Errors
    ///
    /// Return Error emitted by backend
    pub async fn try_receive(&mut self) -> BuqueueResult<Option<Delivery>> {
        self.0.try_receive().await
    }

    /// Returns up to `max` messages
    ///
    /// # Errors
    ///
    /// Return Error emitted by backend
    pub async fn receive_batch(&mut self, max: usize) -> BuqueueResult<Vec<Delivery>> {
        self.0.receive_batch(max).await
    }

    /// Returns the shutdown handle for this consumer
    #[must_use]
    pub fn shutdown_handle(&self) -> ShutdownHandle {
        self.0.shutdown_handle()
    }

    /// Like `receive`, but returns `None` after shutdown is signalled
    pub async fn receive_graceful(&mut self) -> Option<BuqueueResult<Delivery>> {
        self.0.receive_graceful().await
    }
}

impl std::fmt::Debug for BaseDynConsumer<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DynConsumer").finish_non_exhaustive()
    }
}
