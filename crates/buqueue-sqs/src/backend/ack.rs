//! SQS ack handle `DeleteMessage` for ack, `ChangeMessageVisibility` for nack

use std::{future::Future, pin::Pin, sync::Arc};

use aws_sdk_sqs::Client;
use buqueue_core::{
    delivery::AckHandle,
    error::{BuqueueError, BuqueueResult, ErrorKind},
};

use crate::config::SqsMessagMeta;

/// The ack handle attached to every SQS `Delivery`
///
/// ## `ack()`
/// Calls `DeleteMessage` permanently removes the message from the queue
///
/// ## `nack()`
/// Calls `ChangeMessageVisibility(0)`, makes the message immediately visible
/// to other consumers. This is SQS's native requeue mechanism
///
/// ## DLQ
///
/// SQS DLQ routing is handled by a Redrive Policy configured on the queue in
/// AWS Console or `CloudFormation`, not by buqueue. When `AppromixateReceiveCount`
/// exceeds the policy's `maxReceiveCount`, SQS routes the message to the DLQ
/// automatically. Buqueue's `DlqConfig` is stored for documentation only
#[derive(Debug)]
pub(crate) struct SqsAckHandle {
    pub(crate) client: Arc<Client>,
    /// Stored as `String` (cloned from `QueueUrl.as_str()`) because the AWS
    /// SDK builder methods take `impl Into<String>`
    pub(crate) queue_url: String,
    pub(crate) meta: SqsMessagMeta,
}

impl AckHandle for SqsAckHandle {
    fn ack(&self) -> Pin<Box<dyn Future<Output = BuqueueResult<()>> + Send + '_>> {
        Box::pin(async move {
            self.client
                .delete_message()
                .queue_url(&self.queue_url)
                .receipt_handle(self.meta.receipt_handle.as_str())
                .send()
                .await
                .map_err(|e| {
                    BuqueueError::with_source(ErrorKind::AckFailed, e.into_service_error())
                })?;

            tracing::debug!(
                message_id = %self.meta.message_id,
                "SQS message deleted (ack)"
            );
            Ok(())
        })
    }

    fn nack(&self) -> Pin<Box<dyn Future<Output = BuqueueResult<()>> + Send + '_>> {
        Box::pin(async move {
            self.client
                .change_message_visibility()
                .queue_url(&self.queue_url)
                .receipt_handle(self.meta.receipt_handle.as_str())
                .visibility_timeout(0)
                .send()
                .await
                .map_err(|e| {
                    BuqueueError::with_source(ErrorKind::AckFailed, e.into_service_error())
                })?;

            tracing::debug!(
                message_id = %self.meta.message_id,
                receive_count = self.meta.approximate_receive_count,
                "SQS message nacked (visibility timeout set to 0)"
            );
            Ok(())
        })
    }
}
