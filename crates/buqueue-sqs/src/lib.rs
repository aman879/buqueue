//! AWS SQS backend for buqueue
//!
//! Supports Standard and FIFO queues. Queue type is auto-detected from the
//! queue URL, FIFO queues always end in `.fifo`
//!
//! ## Module structure
//!
//! - [`config`] - [`SqsConfig`], [`VisibilityTimeout`]
//! - [`producer`] - [`SqsProducer`]
//! - [`consumer`] - [`SqsConsumer`]
//! - [`builder`] - [`SqsBuilder`] and [`SqsBackend`]
//!
//! ## Feature mapping
//!
//! buqueue concept     :   SQS primitive
//! `routing_key`       :   `MessageGroupId` (FIFO only)
//! `deduplication_id`  :   `MessageDeduplicationId` (FIFO only)
//! `headers`           :   `MessageAttributes`
//! `send_at(delay)`    :   `DelaySeconds` (max 900ms / 15 min)
//! `ack()`             :   `DeleteMessage`
//! `nack()`            :   `ChangeMessageVisibility(0)`
//! `delivery_count`    :   `ApproximateReceiveCount`
//! DLQ routing         :   AWS Redrive Policy (configured on queue)
//!
//! ## Credentials
//!
//! Never stored in config. Loded by the AWS SDK frome env
//! `~/.aws/credentials`, or IAM instance/task roles
//!
//! ## `LocalStack`
//!
//! Set `SqsConfig::endpoint_url = Some("http://localhost:4566".into())`
//! See `test.rs` for setup instructions

#![warn(missing_docs)]
#![forbid(unsafe_code)]
pub mod backend;
pub mod config;
pub mod error;

#[cfg(test)]
mod tests;
