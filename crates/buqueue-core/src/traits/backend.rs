//! The `QueueBackend` and `BackendBuilder` traits
//!
//! In buqueue each backend exposes `builder()` as a plain inherent method:
//!
//! ```rust,ignore
//! impl SqsBackend {
//!     pub fn builder(config: SqsConfig) -> SqsBuilder { ... }
//! }
//!
//! SqsBackend::builder(cfg).build_pair().await?;
//! ```

use crate::prelude::{
    BuqueueResult, DlqConfig, DynConsumer, DynProducer, QueueConsumer, QueueProducer,
};

/// Fluent builder for constructing a backend's producer and/or consumer
///
/// Each backend returns its own concrete builder from an inherent `builder()`
/// method. This trait is what you write generic code
///
/// ```rust,ignore
/// async fn build_test_pair<B>(builder:B) -> (B::Producer, B::Consumer)
/// where
///     B: BackendBuilder,
///     B::Producer: QueueProcuder,
///     B::Consumer: QueueConsumer,
/// {
///     builder.build_pair().await.unwrap()
/// }
/// ```
///
/// ## Type parameters
///
/// - `Producer` - `Send + 'static` only. For conrete builders this implements
///   [`QueueProducer`]. For [`DynamicBuilder`] this is [`DynProducer`], which
///   does not implement `QueueProducer`, it is aready-erased end product
///
/// - `Consumer` - same reasonning as `Producer`
pub trait BackendBuilder: Sized + Send {
    /// Concrete producer type producer by this builder
    type Producer: Send + 'static;
    /// Concrete consumer type produced by this builder
    type Consumer: Send + 'static;

    /// Configure a dead letter queue
    ///
    /// Message nack'd `max_receive_count` times are routed to `destination`
    /// instead of being requeued
    #[must_use]
    fn dead_letter_queue(self, config: DlqConfig) -> Self;

    /// Erase the producer and consumer types, returning a `DynProducer` and
    /// `DynConsumer` pair. Use this when you need to select a backend at
    /// runtime without generic parameters propagation through your application:
    ///
    /// ```rust,ignore
    /// let (producer, consumer): (DynProducer, DynConsumer) =
    ///     match std::env::var("QUEUE_BACKEND").as_deref() {
    ///         Ok("sqs") => SqsBackend::builder(cfg).make_dynamic().build_pair().await?,
    ///         Ok("nats") => NatsBackend::builder(cfg).make_dynamic().build_pair().await?,
    ///         _    => MemoryBackend::builder().make_dynamic().build_pair().await?,
    ///     };
    /// ```
    fn make_dynamic(self) -> DynamicBuilder<Self> {
        DynamicBuilder(self)
    }

    /// Build both a procuder and consumer
    fn build_pair(
        self,
    ) -> impl Future<Output = BuqueueResult<(Self::Producer, Self::Consumer)>> + Send;

    /// Build only a producer
    fn build_producer(self) -> impl Future<Output = BuqueueResult<Self::Producer>> + Send;

    /// Build only a consumer
    fn build_consumer(self) -> impl Future<Output = BuqueueResult<Self::Consumer>> + Send;
}

/// Wraps a `BackendBuilder` to return `DynProducer` / `DynConsumer`
/// instead of concrete backend types.
///
/// Produced by `BackendBuilder::make_dynamic()`
pub struct DynamicBuilder<B>(B);

impl<B: BackendBuilder + Send> BackendBuilder for DynamicBuilder<B>
where
    B::Producer: QueueProducer + 'static,
    B::Consumer: QueueConsumer + 'static,
{
    type Producer = DynProducer;
    type Consumer = DynConsumer;

    fn dead_letter_queue(mut self, config: DlqConfig) -> Self {
        self.0 = self.0.dead_letter_queue(config);
        self
    }

    async fn build_pair(self) -> BuqueueResult<(Self::Producer, Self::Consumer)> {
        let (p, c) = self.0.build_pair().await?;
        Ok((p.into_dyn(), c.into_dyn()))
    }

    async fn build_producer(self) -> BuqueueResult<Self::Producer> {
        Ok(self.0.build_producer().await?.into_dyn())
    }

    async fn build_consumer(self) -> BuqueueResult<Self::Consumer> {
        Ok(self.0.build_consumer().await?.into_dyn())
    }
}
