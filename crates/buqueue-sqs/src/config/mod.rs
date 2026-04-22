//! SQS backend configuration

use std::{fmt, time::Duration};

use aws_config::Region;

/// A valiadated SQS queue URL
///
/// Wraps the raw URL string to prevent accedentally passing a region, or
/// other string where a queue URL is required. Also centralises FIFO
/// detection, any code that has a `QueueUrl` can call `.is_fifo()` witouth
/// inspecting raw string.
///
/// ## Format
///
/// ```text
/// https://sqs.{region}.amazonaws.com/{account-id}/{queue-name}
/// https://sqs.{region}.amazonaws.com/{account-id}/{queue-name}.fifo
///
/// # LocalStack
/// http://localhost:4566/123456789/{queue_name}
/// ```
///
/// ## Construction
///
/// ```rust
/// use buqueue_sqs::config::QueueUrl;
///
/// let url = QueueUrl::new("https://sqs.us-east-1.amazonaws.com/123456789/orders");
/// assert!(!url.is_fifo());
///
/// let fifo = QueueUrl::new("https://sqs.us-east-1.amazonaws.com/123456789/orders.fifo");
/// assert!(fifo.is_fifo());
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct QueueUrl(String);

impl QueueUrl {
    /// Wrap a raw URL string. No network call is made, the url is not verified
    /// to exist. Validation happens when the AWS SDK first uses it.
    pub fn new(url: impl Into<String>) -> Self {
        Self(url.into())
    }

    /// Returns `true` if this is a FIFO queue (URL ends in `.fifo`).
    ///
    /// SQS FIFO queues always have this suffix, it is enforced by AWS
    #[allow(clippy::case_sensitive_file_extension_comparisons)]
    #[must_use]
    pub fn is_fifo(&self) -> bool {
        self.0.ends_with(".fifo")
    }

    /// Returns the raw URL string slice
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for QueueUrl {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<&str> for QueueUrl {
    fn from(value: &str) -> Self {
        Self::new(value)
    }
}

impl From<String> for QueueUrl {
    fn from(value: String) -> Self {
        Self::new(value)
    }
}

impl AsRef<str> for QueueUrl {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

/// An opaque SQS receipt handle used to ack or nack a specific delivery
///
/// SQS receipt handles are long, URL-encoded strings that identify a specific
/// delivery of a message. They are not the same as the message ID, a new handle
/// is issued every time SQS devliers the message
///
/// Making this a newtype prevents accidentally passing a `message_id` or queue
/// URL where a receipt handle is required, since all three are `String` in the
/// raw SDK type
#[derive(Debug, Clone)]
pub struct ReceiptHandle(pub(crate) String);

#[allow(dead_code)]
impl ReceiptHandle {
    pub(crate) fn new(s: impl Into<String>) -> Self {
        Self(s.into())
    }

    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ReceiptHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = &self.0;
        if s.len() > 20 {
            write!(f, "{}...", &s[..20])
        } else {
            writeln!(f, "{s}")
        }
    }
}

/// Configuration for the SQS backend
///
/// ## Credentials
///
/// Never put credentials in this struct. The AWS SDK reads them from:
/// 1. `AWS_ACCESS_KEY_ID` / `AWS_SECRET_ACCESS_KEY` environment vaiables
/// 2. `~/.aws/credentials` file
/// 3. IAM instance/task role (EC2, ECS, lambda)
///
/// For Localstack, set `endpoint_url` and the SDK will use whatever
/// dummy credentials are in the environment
///
/// ## Queue mode detection
///
/// The queue mode (Standard bs FIFO) is detected automatically from the
/// `queue_url`, FIFO queues always end in `.fifo`. You do not need to
/// set it explicitly
///
/// ## Example
///
/// ```rust,ignore
/// use buqueue_sqs::config::{SqsConfig, QueueUrl};
///
/// let config = SqsConfig {
///     queue_url: QueueUrl::new("https://sqs.us-east-1.amazonaws.com/123456789/orders"),
///     ..Default::default()
/// };
///
/// // LocalStack:
/// let config = SqsConfig {
///     queue_url: QueueUrl::new("http://localhost:4556/123456789/orders"),
///     region: "us-east-1".into(),
///     endpoint_url: Some("http://localhost:4566".into()),
///     ..Default::default()
/// };
/// ```
#[derive(Debug, Clone)]
pub struct SqsConfig {
    /// Full SQS queue URL, typed as `QueueUrl` to prevent string confusion
    pub queue_url: QueueUrl,

    /// AWS region. Uses `aws_config::Region` directly, the correct SDK type
    /// rather than a plain `String`
    pub region: Region,

    /// Override the SQS endpoint URL
    ///
    /// Set to `"http://localhost:4566"` for `LocalStack`.
    /// Leave `None` for real AWS
    pub endpoint_url: Option<String>,

    /// How long SQS hides a received messafe from other consumer.
    ///
    /// Defaults to `VisibilityTimeout::Seconds(30)`. Maximum is 43200s (12h)
    pub visibility_timeout: VisibilityTimeout,

    /// How long `receive()` waits before returning empty
    ///
    /// Values 1-20 seconds, Higer = fewer empty responses = lower cost
    /// Default to 20 seconds
    pub wait_time_seconds: u32,

    /// Maximum messages per `receive_batch()` call. SQS maximum is 10
    pub max_batch_size: u32,
}

impl Default for SqsConfig {
    fn default() -> Self {
        Self {
            queue_url: QueueUrl::new(""),
            region: Region::new("us-east-1"),
            endpoint_url: None,
            visibility_timeout: VisibilityTimeout::Seconds(30),
            wait_time_seconds: 20,
            max_batch_size: 10,
        }
    }
}

/// How long SQS hides a received messafe from other consumer.
#[derive(Debug, Clone, Copy)]
pub enum VisibilityTimeout {
    /// Explicit timeout in seconds. Maximum is 43200 (12 hours)
    Seconds(u32),
}

impl VisibilityTimeout {
    /// Returns the timeout as a raw second count for the AWS SDK
    #[must_use]
    pub fn as_secs(&self) -> u32 {
        match self {
            Self::Seconds(s) => *s,
        }
    }

    /// Returns the timeout as a `Duration`
    #[must_use]
    pub fn as_duration(&self) -> Duration {
        Duration::from_secs(u64::from(self.as_secs()))
    }
}

/// SQS specific metadata attached to each received message
///
/// Used internally by `SqsAckHandle`, not exposed to users directly
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub(crate) struct SqsMessagMeta {
    /// Typed receipt handle, required for `DeleteMessage` and `ChangeMessageVisibility`
    pub(crate) receipt_handle: ReceiptHandle,
    /// How many times SQS has delivered this message
    pub(crate) approximate_receive_count: u32,
    /// SQS-assigned message ID (not same as buqeue's `MessageId`)
    pub(crate) message_id: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn queue_url_fifo_detection() {
        assert!(!QueueUrl::new("https://sqs.us-east-1.amazonaws.com/123/orders").is_fifo());
        assert!(QueueUrl::new("https://sqs.us-east-1.amazonaws.com/123/orders.fifo").is_fifo());
        assert!(QueueUrl::new("http://localhost:4566/000000000000/test.fifo").is_fifo());
    }

    #[test]
    fn queue_url_display() {
        let url = QueueUrl::new("https://sqs.us-east-1.amazonaws.com/123/orders");
        assert_eq!(
            url.to_string(),
            "https://sqs.us-east-1.amazonaws.com/123/orders"
        );
    }

    #[test]
    fn queue_url_from_str() {
        let url: QueueUrl = "https://sqs.us-east-1.amazonaws.com/123/orders".into();
        assert!(!url.is_fifo());
    }

    #[test]
    fn receipt_handle_display_truncates() {
        let handle = ReceiptHandle::new("a".repeat(100));
        let displayed = handle.to_string();
        assert!(displayed.ends_with("..."));
        assert!(displayed.len() < 30);
    }

    #[test]
    fn visibility_timeout_conversions() {
        let vt = VisibilityTimeout::Seconds(30);
        assert_eq!(vt.as_secs(), 30);
        assert_eq!(vt.as_duration(), Duration::from_secs(30));
    }
}
