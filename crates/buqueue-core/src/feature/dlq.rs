//! Dead Letter Queue configuration
//!
//! Pass a `DlqConfig` to any backend builder bia `.dead_letter_queue(config)`
//! to enable automatic routing of failed messages to a dead letter destination

/// Configuration for Dlq behavior
///
/// When a message has been delivered and nack'd `max_receive_count` times,
/// buqeueu routes it to the `destination` instead of requeueing it.
///
/// ## Usage
///
/// ```rust,ignore
/// let (producer, cosumer) = SqsBackend::builder(config)
///     .dead_letter_queue(DlqConfig{
///         destination: "https://sqs.../orders-dlq".to_string(),
///         max_receive_count: 5,
///     })
///     .build_pair()
///     .await?;
/// ```
///
/// ## Backend mapping
///
/// SQS -> The DLQ queue URL
/// NATS -> The `JetStream` stream to route to
/// `RabbitMQ` -> The dead letter exchange name
/// Redis -> The dead letter stream key
/// Kafka -> The dead letter topic name
#[derive(Debug, Clone)]
pub struct DlqConfig {
    /// Where failed message are sent
    ///
    /// The format depends on the backend
    pub destination: String,

    /// How many total delivery attempts are made before routing to the DLQ
    ///
    /// A value of `5` means: one original delivery + 4 redeliveries
    /// On the 5th nack, the message goes to `destination`
    ///
    /// Must be at least 1. The recommended value for most workloads is 3-5
    pub max_receive_count: u32,
}

impl DlqConfig {
    /// Creates a new `DlqConfig`.
    ///
    /// # Panics
    /// if `max_receive_count` is less than 1
    #[must_use]
    pub fn new(destination: String, max_receive_count: u32) -> Self {
        assert!(
            max_receive_count >= 1,
            "max_receive_count must be at least 1"
        );
        Self {
            destination,
            max_receive_count,
        }
    }
}
