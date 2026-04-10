//! Error type for buqueue
//!
//! The central type is `BuqueueError`, which wraps and `ErrorKind` variant.
//! The `ErrorKind` lets callers distinguish transient failures (safe to retry)
//! from permanent ones (worh alerting on) withouth matching on error message strings.

use thiserror::Error;

// Rust 2024 edition: no changes needed to error types specifically,
// byt we use thiserror's derive which works identically.

/// Convenience type alias, `BuqueueResult<T>` is `Result<T, BuqueueError>`.
///
/// The error type is always `BuqueueError`, there's not second type parameter.
/// If you need to carry a different error type, convert it to `BuqueueError`  first
/// using `BuqueueError::with_source`.
pub type BuqueueResult<T, E = BuqueueError> = Result<T, E>;

/// The top-level error type returned by all buqueue operations
///
/// Match on `.kind` to handle specific failure modes:
#[derive(Debug, Error)]
#[error("{kind}")]
pub struct BuqueueError {
    /// What kind of error occurred. Use this for matching
    pub kind: ErrorKind,

    /// Optional human-readable context string.
    /// Not intended for programmatic matching
    #[source]
    pub source: Option<Box<dyn std::error::Error + Send + Sync>>,
}

impl BuqueueError {
    /// Construct a `BuqueueError` with a kind and no source.
    #[must_use]
    pub fn new(kind: ErrorKind) -> Self {
        Self { kind, source: None }
    }

    /// Construct a `BuqueueError` with a kind and a source error for context.
    #[must_use]
    pub fn with_source(
        kind: ErrorKind,
        source: impl std::error::Error + Send + Sync + 'static,
    ) -> Self {
        Self {
            kind,
            source: Some(Box::new(source)),
        }
    }

    /// Returns `true` if this error is transient and the operation is safe to retry.
    ///
    /// Transient errors: `ConnectionLost`, `Timeout`, `BrokerUnavailable`.
    /// Permanent errors: all others.
    #[must_use]
    pub fn is_transient(&self) -> bool {
        matches!(
            self.kind,
            ErrorKind::ConnectionLost | ErrorKind::Timeout | ErrorKind::BrokerUnavailable
        )
    }
}

/// Classifies the kind of failure that occurred.
///
/// This is the primary surface for programmatic error handling.
/// New variants may be added in minor versions, always include a catch-all arm.
#[derive(Debug, Error, PartialEq, Eq)]
#[non_exhaustive]
pub enum ErrorKind {
    // ----------------------------------------------------------------
    // Transient errors (safe to retry)
    // ----------------------------------------------------------------
    /// The connection to the broker was lost mid-operation.
    ///
    /// buqueue will attempt to reconnect automatically in the background.
    /// You may also retry the failed operation after a short backoff.
    #[error("connection to broker was lost")]
    ConnectionLost,

    /// The operation timed out waiting for a broker response.
    #[error("operation timed out")]
    Timeout,

    /// The broker is temporarily unavailable (overloaded, restarting, etc).
    #[error("broker is temporarily unavailable")]
    BrokerUnavailable,

    // ----------------------------------------------------------------
    // Permanent errors - do not retry
    // ----------------------------------------------------------------
    /// The message payload exceeds the broker's maximum message size.
    ///
    /// This will never succeed regardless of retires.
    /// Log the message, split it, or route it to an error channel
    #[error("message payload is too large for the broker")]
    PayloadTooLarge,

    /// Credentials were rejected by the broker.
    ///
    /// Check your configuration, rotating credentials or re-authentication
    /// is required before any further operations will succeed.
    #[error("authentication failed, check broker credentials")]
    AuthenticationFailed,

    /// The provided configuration is invalid
    ///
    /// The error message contains a description of what is wrong.
    #[error("invalid configuration: {0}")]
    InvalidConfig(String),

    /// The requested queue, topic, stram, or exchange does not exist
    /// and buqueue is not configured to create it automatically.
    #[error("queue or topic is not found: {0}")]
    NotFound(String),

    /// The payload could not be deserialised into the requested type.
    ///
    /// The payload bytes are structurally valid (the broker accepted them) but
    /// cannot be decoded as the target Rust type. This usually means a schema
    /// mismatch. Check `Schema-version` header if you use one.
    #[error("payload deserialization failed: {0}")]
    DeserializationFailed(String),

    /// The payload could not be serialised.
    #[error("payload serialisation failed: {0}")]
    SerializationFailed(String),

    /// Acknowledgement failed.
    ///
    /// The message was processed but the broker could not be told about it.
    /// The message will likely be redelivered. Your consumer should be
    /// idempotent to handle this case.
    #[error("acknowledgement failed")]
    AckFailed,

    /// The consumer has been shut down and will not deliver more messages.
    #[error("consumer has been shut down")]
    ConsumerShutdown,

    /// A backend-specific error that buqueue does not classfiy more precisely.
    ///
    /// `code` is the raw backend error code (where available).
    /// `message` is a human-readable description.
    ///
    /// ## Contructing this vairant
    ///
    /// ```rust
    /// # use buqueue_core::error::ErrorKind;
    /// // With a code (e.g. Kafka error code, AMQP reply code):
    /// let e = ErrorKind::BackendSpecific {
    ///     code: Some("TOPIC_AUTHROZATION_FAILED".into()),
    ///     message: "not authorised to producde to topic 'orders'".into(),
    /// };
    ///
    /// // Without a code (e.g. a raw I/O error with no structured classification):
    /// let e = ErrorKind::BackendSpecific {
    ///     code: None,
    ///     message: "unexpected EOF on socket".into()
    /// };
    /// ```
    #[error("backend error{}: {message}", code.as_deref().map(|c| format!(" (code={c})")).unwrap_or_default())]
    BackendSpecific {
        /// Raw error code from the backend, if available.
        code: Option<String>,
        /// Human-readable error message.
        message: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transient_errors_classified_correctly() {
        assert!(BuqueueError::new(ErrorKind::ConnectionLost).is_transient());
        assert!(BuqueueError::new(ErrorKind::Timeout).is_transient());
        assert!(BuqueueError::new(ErrorKind::BrokerUnavailable).is_transient());
    }

    #[test]
    fn permanent_errors_are_not_transient() {
        assert!(!BuqueueError::new(ErrorKind::PayloadTooLarge).is_transient());
        assert!(!BuqueueError::new(ErrorKind::AuthenticationFailed).is_transient());
        assert!(!BuqueueError::new(ErrorKind::AckFailed).is_transient());
    }

    #[test]
    fn backend_specific_display_with_code() {
        let e = BuqueueError::new(ErrorKind::BackendSpecific {
            code: Some("TOPIC_AUTH_FAILED".into()),
            message: "not authorised".into(),
        });

        assert_eq!(
            e.to_string(),
            "backend error (code=TOPIC_AUTH_FAILED): not authorised"
        );
    }

    #[test]
    fn backend_specific_display_without_code() {
        let e = BuqueueError::new(ErrorKind::BackendSpecific {
            code: None,
            message: "unexpected EOF".into(),
        });

        assert_eq!(e.to_string(), "backend error: unexpected EOF");
    }
}
