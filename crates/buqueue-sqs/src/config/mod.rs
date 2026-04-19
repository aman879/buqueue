//! SQS backend configuration

use std::fmt;

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
/// asser!(!url.is_fifo());
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
/// let config = SqsConfig {
///     queue_url: "https://sqs.us-east-1.amazonaws.com/123456789/orders".into(),
///     region: "us-east-1".into(),
///     ..Default::default()
/// };
/// 
/// // LocalStack:
/// let config = SqsConfig {
///     queue_url: "http://localhost:4556/123456789/orders".into(),
///     region: "us-east-1".into(),
///     endpoint_url: Some("http://localhost:4566".into()),
///     ..Default::default()
/// };
/// ```
#[derive(Debug, Clone)]
pub struct SqsConfig {

}