//! SQS producer implementation

use std::{collections::HashMap, sync::Arc};

use aws_sdk_sqs::{Client, types::MessageAttributeValue};
use buqueue_core::{
    error::{BuqueueError, BuqueueResult, ErrorKind},
    prelude::{Message, MessageId, QueueProducer},
};
use chrono::{DateTime, Utc};

use crate::{config::SqsConfig, error::sdk_err};

/// SQS producer
///
/// Cheaply cloneable via `Arc<Client>`, share across tasks freely
///
/// ## FIFO queues
///
/// On FIFO queues:
/// - `message.routing_key()` mapts to `MessageGroupId` (required for FIFO)
/// - `message.deduplication_id()` maps to `MessageDeduplicationId`
///   required unless the queue has content based deduplication enabled
#[derive(Debug, Clone)]
pub struct SqsProducer {
    pub(crate) client: Arc<Client>,
    pub(crate) config: Arc<SqsConfig>,
}

impl QueueProducer for SqsProducer {
    async fn send(&self, message: Message) -> BuqueueResult<MessageId> {
        let body = payload_to_str(&message)?;
        let mut req = self
            .client
            .send_message()
            .queue_url(self.config.queue_url.as_str())
            .message_body(body);

        req = attach_headers(req, &message)?;

        if self.config.queue_url.is_fifo() {
            req = attach_fifo_fields(req, &message);
        }

        let out = req.send().await.map_err(sdk_err)?;
        let id = out.message_id().unwrap_or("unknown").to_string();

        tracing::debug!(
            message_id = %id,
            queue_url = %self.config.queue_url,
            "SQS message sent"
        );

        Ok(MessageId::from(id))
    }

    /// Sends upto 10 messages per `SendMessageBatch` call
    ///
    /// SQS hard limit is 10 messages nd 256 KB total payload per batch
    /// Inputs larger then 1- are split into chunks automatically
    /// A partial failure returns an error immediatrly
    async fn send_batch(&self, messages: Vec<Message>) -> BuqueueResult<Vec<MessageId>> {
        use aws_sdk_sqs::types::SendMessageBatchRequestEntry;
        use uuid::Uuid;

        let mut all_ids = Vec::with_capacity(messages.len());

        for chunk in messages.chunks(10) {
            let mut entries = Vec::with_capacity(chunk.len());

            for msg in chunk {
                let entry_id = Uuid::new_v4().to_string();
                let body = payload_to_str(msg)?;

                let mut b = SendMessageBatchRequestEntry::builder()
                    .id(&entry_id)
                    .message_body(body);

                b = attach_batch_headers(b, msg)?;

                if self.config.queue_url.is_fifo() {
                    b = attach_batch_fifo_fields(b, msg);
                }

                entries.push(b.build().map_err(|e| {
                    BuqueueError::with_source(
                        ErrorKind::BackendSpecific {
                            code: None,
                            message: "failed to build batch entry".into(),
                        },
                        e,
                    )
                })?);
            }

            let out = self
                .client
                .send_message_batch()
                .queue_url(self.config.queue_url.as_str())
                .set_entries(Some(entries))
                .send()
                .await
                .map_err(sdk_err)?;

            if !out.failed().is_empty() {
                let f = &out.failed()[0];

                return Err(BuqueueError::new(ErrorKind::BackendSpecific {
                    code: Some(f.code().to_string()),
                    message: format!(
                        "{} of {} messages failed to batch. First: {}",
                        out.failed().len(),
                        chunk.len(),
                        f.message().unwrap_or("unknown"),
                    ),
                }));
            }

            let id_map: HashMap<_, _> = out
                .successful()
                .iter()
                .map(|s| (s.id().to_string(), s.message_id()))
                .collect();

            for entry in out.successful() {
                all_ids.push(MessageId::from(
                    id_map.get(entry.id()).copied().unwrap_or("unknown"),
                ));
            }
        }

        Ok(all_ids)
    }

    /// Send a message that becomes visible after `deliver_at`
    ///
    /// SQS supports up to 900 seconds (15 minutes) natively via `DelaySeconds`
    /// Longer delays are clamped to 900 s with a `tracing::warn!`, use a
    /// holding queue or `EventBridge` Scheduler for longer delays.
    /// Sub-second precision is lost
    async fn send_at(
        &self,
        message: Message,
        delivery_at: DateTime<Utc>,
    ) -> BuqueueResult<MessageId> {
        let now = Utc::now();
        let delay = if delivery_at > now {
            (delivery_at - now)
                .to_std()
                .unwrap_or(std::time::Duration::ZERO)
        } else {
            std::time::Duration::ZERO
        };

        let raw_secs = delay.as_secs();
        let delay_secs = raw_secs.min(900) as i32;

        if raw_secs > 900 {
            tracing::warn!(
                delay_secs = raw_secs,
                "SQS max delay is 900 s. Clamping. Use a holding queue for longer delays"
            );
        }

        let body = payload_to_str(&message)?;

        let mut req = self
            .client
            .send_message()
            .queue_url(self.config.queue_url.as_str())
            .message_body(body)
            .delay_seconds(delay_secs);

        req = attach_headers(req, &message)?;

        if self.config.queue_url.is_fifo() {
            req = attach_fifo_fields(req, &message);
        }

        let out = req.send().await.map_err(sdk_err)?;
        let id = out.message_id().unwrap_or("unknown").to_string();

        tracing::debug!(message_id = %id, delay_secs, "SQS scheduled message sent");

        Ok(MessageId::from(id))
    }
}

fn payload_to_str(msg: &Message) -> BuqueueResult<&str> {
    std::str::from_utf8(msg.payload()).map_err(|e| {
        BuqueueError::with_source(
            ErrorKind::SerializationFailed("payload is not a valid UTD-8".into()),
            e,
        )
    })
}

fn build_attr(key: &str, value: &str) -> BuqueueResult<MessageAttributeValue> {
    MessageAttributeValue::builder()
        .data_type("String")
        .string_value(value)
        .build()
        .map_err(|e| {
            BuqueueError::with_source(
                ErrorKind::BackendSpecific {
                    code: None,
                    message: format!("failed to build MessageAttribute for key {key:?}"),
                },
                e,
            )
        })
}

type SendReq = aws_sdk_sqs::operation::send_message::builders::SendMessageFluentBuilder;

fn attach_headers(mut req: SendReq, msg: &Message) -> BuqueueResult<SendReq> {
    for (key, value) in msg.headers() {
        req = req.message_attributes(key, build_attr(key, value)?);
    }
    Ok(req)
}

fn attach_fifo_fields(mut req: SendReq, msg: &Message) -> SendReq {
    if let Some(group_id) = msg.routing_key() {
        req = req.message_group_id(group_id);
    }
    if let Some(dedup_id) = msg.deduplication_id() {
        req = req.message_deduplication_id(dedup_id);
    }
    req
}

type BatchEntryBuilder = aws_sdk_sqs::types::builders::SendMessageBatchRequestEntryBuilder;

fn attach_batch_headers(
    mut b: BatchEntryBuilder,
    msg: &Message,
) -> BuqueueResult<BatchEntryBuilder> {
    for (key, value) in msg.headers() {
        b = b.message_attributes(key, build_attr(key, value)?);
    }
    Ok(b)
}

fn attach_batch_fifo_fields(mut b: BatchEntryBuilder, msg: &Message) -> BatchEntryBuilder {
    if let Some(group_id) = msg.routing_key() {
        b = b.message_group_id(group_id);
    }

    if let Some(dedup_id) = msg.deduplication_id() {
        b = b.message_deduplication_id(dedup_id);
    }
    b
}
