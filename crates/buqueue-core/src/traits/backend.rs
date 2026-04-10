//! The `QueueBackend` and `BackendBuilder` traits

use crate::prelude::{
    BuqueueResult, DlqConfig, DynConsumer, DynProducer, QueueConsumer, QueueProducer,
};

/// A backend that can produce and consume messages
///
/// Every buqueue backend implements this trait. The pattern is:
///
/// ```rust,ignore
/// let(producer, consumer) = MyBackend::builder(config)
///     .dead_letter_queue(dlq_config)
///     .build_pair()
///     .await?;
/// ```
pub trait QueueBackend: Sized {
    /// Backend-specific configuration type (e.g. `KafkaConfig`, `SqsConfig`)
    type Config;
    /// Concrete producer type
    type Producer: QueueProducer + 'static;
    /// Concrete consumer type
    type Consumer: QueueConsumer + 'static;
    /// Builder type returned by `builder()`
    type Builder: BackendBuilder<Producer = Self::Producer, Consumer = Self::Consumer>;

    /// Start configuring the backend
    fn builder(config: Self::Config) -> Self::Builder;
}

/// Fluent builder returned by `QueueBackend::builder()`
pub trait BackendBuilder: Sized + Send {
    /// Concrete producer type producer by this builder
    type Producer: Send + 'static;
    /// Concrete consumer type produced by this builder
    type Consumer: Send + 'static;

    /// Configure a dead letter queue
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
