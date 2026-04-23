//! SQS conumer, long poll receive, native btach receive, gracefult shutdown

use std::{collections::HashMap, sync::Arc};

use aws_sdk_sqs::{Client, types::MessageSystemAttributeName};
use buqueue_core::{
    error::{BuqueueError, BuqueueResult, ErrorKind},
    prelude::{Delivery, QueueConsumer, ShutdownHandle},
};
use bytes::Bytes;

use crate::{
    backend::ack::SqsAckHandle,
    config::{ReceiptHandle, SqsConfig, SqsMessagMeta},
    error::sdk_err,
};

/// SQS consumer
///
/// Uses SQS long polling (`WaitTimeSeconds=20` by default), `receive()` blocks
/// upto 20 seconds waiting for a message. This is the recommended approach:
/// it reduces empty responses and lowers API costs vs short polling
///
/// ## Visibility timeout
///
/// After `receive()` return a `Delivery`, SQS hides that message for
/// `visibility_timout` seconds. You must call `ack()` or `nack()` before the
/// timeout expires or SQS delivery automatically
#[derive(Debug)]
pub struct SqsConsumer {
    pub(crate) client: Arc<Client>,
    pub(crate) config: Arc<SqsConfig>,
    pub(crate) shutdown: ShutdownHandle,
}

impl SqsConsumer {
    fn make_delivery(&self, msg: &aws_sdk_sqs::types::Message) -> BuqueueResult<Delivery> {
        let receipt_handle = msg
            .receipt_handle()
            .ok_or_else(|| {
                BuqueueError::new(ErrorKind::BackendSpecific {
                    code: None,
                    message: "SQS message missing recepit handle, cannot ack/nack".into(),
                })
            })
            .map(ReceiptHandle::new)?;

        let message_id = msg.message_id().unwrap_or("unkown").to_string();

        let receive_count: u32 = msg
            .attributes()
            .and_then(|a| a.get(&MessageSystemAttributeName::ApproximateReceiveCount))
            .and_then(|v| v.parse().ok())
            .unwrap_or(1);

        let mut headers = HashMap::new();

        if let Some(attrs) = msg.message_attributes() {
            for (key, attr) in attrs {
                if let Some(v) = attr.string_value() {
                    headers.insert(key.clone(), v.to_string());
                }
            }
        }

        headers.insert("bg-sqs-message-id".into(), message_id.clone());
        headers.insert("bq-sqs-receive-count".into(), receive_count.to_string());

        let routing_key = msg
            .attributes()
            .and_then(|a| a.get(&MessageSystemAttributeName::MessageGroupId))
            .cloned();

        let payload = Bytes::from(msg.body().unwrap_or("").as_bytes().to_vec());

        let ack_handle = Arc::new(SqsAckHandle {
            client: Arc::clone(&self.client),
            queue_url: self.config.queue_url.as_str().to_string(),
            meta: SqsMessagMeta {
                receipt_handle,
                message_id,
                approximate_receive_count: receive_count,
            },
        });

        Ok(Delivery::new(
            payload,
            headers,
            routing_key,
            receive_count,
            None,
            ack_handle,
        ))
    }

    fn poll_builder(
        &self,
        max: i32,
    ) -> aws_sdk_sqs::operation::receive_message::builders::ReceiveMessageFluentBuilder {
        self.client
            .receive_message()
            .queue_url(self.config.queue_url.as_str())
            .max_number_of_messages(max)
            .wait_time_seconds(self.config.wait_time_seconds.cast_signed())
            .visibility_timeout(self.config.visibility_timeout.as_secs().cast_signed())
            .message_system_attribute_names(MessageSystemAttributeName::All)
            .message_attribute_names("All")
    }

    /// Like `poll_builder` but with `WaitTimeSeconds=0` returns immediately
    ///
    /// Used by `try_receive`. Note: SQS short polling only samples a subset of
    /// servers, So `None` doesnt gaurantee the queue is truly empty
    fn try_poll_builder(
        &self,
        max: i32,
    ) -> aws_sdk_sqs::operation::receive_message::builders::ReceiveMessageFluentBuilder {
        self.client
            .receive_message()
            .queue_url(self.config.queue_url.as_str())
            .max_number_of_messages(max)
            .wait_time_seconds(0)
            .visibility_timeout(self.config.visibility_timeout.as_secs().cast_signed())
            .message_system_attribute_names(MessageSystemAttributeName::All)
            .message_attribute_names("All")
    }

    /// Returns the approximate number of message in the queue.
    ///
    /// Calls `GetQueueAttributes` with:
    /// - `AppromixateNumberOfMessages`: vsible messages waiting to be received
    /// - `ApproximateNumberOfMessageNotVisible`: messages currently waiting
    ///
    /// Use this when you need a reliable emptiness check that `try_receive`
    /// cannot provide due to SQS short polling's sampling nature.
    ///
    /// ## Example
    ///
    /// ```rust,ignore
    /// let depth = consumer.approximate_message_count().await?;
    /// println!("visible={} waiting={}", depth.visible, depth.in_flight);
    ///
    /// if depth.is_empty() {
    ///    println!("queue is empty");
    /// ```
    ///
    /// ## Note on accuracy
    ///
    /// SQS counts are appromixate and may lag by a few seconds. The words
    /// "approximate" in the attribure name is AWS's own caveat. Counts are
    /// eventually consistent, not transactionally exact.
    ///
    /// ## Errors
    ///
    /// `GetQueueAttributes` may fail due to transient network or AWS issues.
    /// In that case, this method returns an error. You can choose to retry or treat
    /// it as an empty queue based on your application's needs.
    pub async fn approximate_message_count(&self) -> BuqueueResult<QueueDepth> {
        use aws_sdk_sqs::types::QueueAttributeName;

        let out = self
            .client
            .get_queue_attributes()
            .queue_url(self.config.queue_url.as_str())
            .attribute_names(QueueAttributeName::ApproximateNumberOfMessages)
            .attribute_names(QueueAttributeName::ApproximateNumberOfMessagesNotVisible)
            .send()
            .await
            .map_err(sdk_err)?;

        let attrs = out.attributes().cloned().unwrap_or_default();

        let visible = attrs
            .get(&QueueAttributeName::ApproximateNumberOfMessages)
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(0);

        let in_flight = attrs
            .get(&QueueAttributeName::ApproximateNumberOfMessagesNotVisible)
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(0);

        tracing::debug!(
            queue_url = %self.config.queue_url,
            visible,
            in_flight,
            "SQS queue depth"
        );

        Ok(QueueDepth { visible, in_flight })
    }
}

/// The approximate number of messags in an SQS queue
///
/// Returned by `SqsConsumer::approximate_message_count()`
/// Both counts are approximate SQS attributes are eventually consistent
#[derive(Debug, Clone)]
pub struct QueueDepth {
    /// Messages visible and waiting to be received
    pub visible: u64,
    /// Messages in-flight, received but not yet acked/nacked
    pub in_flight: u64,
}

impl QueueDepth {
    /// Returns `true` if both `visible` and `in_flight` are zero
    ///
    /// Due to eventual consistency this may briefly return `true` while  a
    /// message is in transit between state. Use with appropriate tolerance
    /// in time sensitive logic
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.visible == 0 && self.in_flight == 0
    }

    /// Total messges
    #[must_use]
    pub fn total(&self) -> u64 {
        self.visible + self.in_flight
    }
}

impl QueueConsumer for SqsConsumer {
    async fn receive(&mut self) -> BuqueueResult<Delivery> {
        loop {
            if self.shutdown.is_shutdown() {
                return Err(BuqueueError::new(ErrorKind::ConsumerShutdown));
            }

            let shutdown = self.shutdown.clone();

            let result = tokio::select! {
                biased;
                () = shutdown.wait_for_shutdown() => {
                    return Err(BuqueueError::new(ErrorKind::ConsumerShutdown));
                }
                r = self.poll_builder(1).send() => r,
            };

            let out = result.map_err(sdk_err)?;

            if out.messages().is_empty() {
                tracing::trace!(
                    queue_url = %self.config.queue_url,
                    "SQS long poll returned empty"
                );
                continue;
            }

            return self.make_delivery(&out.messages()[0]);
        }
    }

    /// Poll SQS withouth blocking, returns immediately
    ///
    /// Uses `WaitTimeSeconds=0` so it may never blocks
    ///
    /// ## Important SQS caveat
    ///
    /// SQS short polling only samples a random subset of the servers holding
    /// your queue's partitions. A `None` result doesnt guarantee the
    /// queue is truly empty, there may be messages on un-sampled servers.
    /// Use `try_receive` for best-effort checks but never reply on it
    /// for deifnitive emptiness assertiongs in production
    ///
    /// For reliable empty queue detection in production, use `SqsConsumer::approximate_message_count()`
    /// and inspect `QueueDepth::is_empty()`. It calls `GetQueueAttributes` which
    /// gives an eventuallt consistent count across all paritions
    async fn try_receive(&mut self) -> BuqueueResult<Option<Delivery>> {
        if self.shutdown.is_shutdown() {
            return Err(BuqueueError::new(ErrorKind::ConsumerShutdown));
        }

        let out = self.try_poll_builder(1).send().await.map_err(sdk_err)?;

        match out.messages().first() {
            Some(msg) => Ok(Some(self.make_delivery(msg)?)),
            None => Ok(None),
        }
    }

    /// Receive up to `max` messages in a single `ReceiveMessage` call
    ///
    /// SQS cap this at 10 regardless of `max`. For more then 10, call this
    /// method multiple time
    async fn receive_batch(&mut self, max: usize) -> BuqueueResult<Vec<Delivery>> {
        if self.shutdown.is_shutdown() {
            return Err(BuqueueError::new(ErrorKind::ConsumerShutdown));
        }

        let count = i32::try_from(max.clamp(1, 10)).map_err(|e| {
            BuqueueError::with_source(
                ErrorKind::BackendSpecific {
                    code: None,
                    message: "batch size must be between 1 and 10".into(),
                },
                e,
            )
        })?;

        let out = self.poll_builder(count).send().await.map_err(sdk_err)?;

        if out.messages().is_empty() {
            return Ok(vec![self.receive().await?]);
        }

        out.messages()
            .iter()
            .map(|m| self.make_delivery(m))
            .collect()
    }

    fn shutdown_handle(&self) -> ShutdownHandle {
        self.shutdown.clone()
    }

    async fn receive_graceful(&mut self) -> Option<BuqueueResult<Delivery>> {
        if self.shutdown.is_shutdown() {
            return None;
        }

        match self.receive().await {
            Err(BuqueueError {
                kind: ErrorKind::ConsumerShutdown,
                ..
            }) => None,
            result => Some(result),
        }
    }
}
