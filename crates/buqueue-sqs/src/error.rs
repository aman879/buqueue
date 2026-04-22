//! Internal SDK error conversion utilities
//! 
//! `sdk_err` is the single place that converts an `aws_sdk_sqs:error::SdkError`
//! into a `BuqueueError`. Both producer and consumer import it from here


use aws_sdk_sqs::error::{ProvideErrorMetadata, SdkError};
use buqueue_core::error::{BuqueueError, ErrorKind};

/// Convert any `SdkError<E>` into a `BuqueueError`.
/// 
/// The bounds required by the AWS SDK to call `.code()` and `.message()` on
/// an `SdkError` are:
/// 
/// - `E: ProvideErrorMetadata` - gives access to `.code()` and `.message()`
///   on the service error vairant
/// - `E: std::error::Error + Send + Sync + 'static`, required by
///   `BuqueueError::with_source` to box the error as a source
/// 
/// `ProvideErrorMetadata` is implemented on `SdkError<E, R>` when
/// `E: ProvideErrorMetadata`, so the call is
///     `e.code()` -> `Option<&str>` from the service error's metadta
///     `e.message()` -> `Option<&str>` from the service error's metadta
pub(crate) fn sdk_err<E, R>(e: SdkError<E, R>) -> BuqueueError
where 
    E: ProvideErrorMetadata + std::error::Error + Send + Sync + 'static,
    R: std::fmt::Debug + Send + Sync + 'static,
{
    let code = e.code().map(str::to_string);
    let message = e.message()
        .map(str::to_string)
        .unwrap_or_else(|| e.to_string());

    BuqueueError::with_source(
        ErrorKind::BackendSpecific { code, message },
        e,
    )
}