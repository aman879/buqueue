//! Common imports for buqueue-core
//!
//! Instead of importing multiple modules, you can do:
//!
//! ```rust
//! use buqueue_core::prelude::*;
//! ```

// Core types
pub use crate::core::delivery::Delivery;
pub use crate::core::error::{BuqueueError, BuqueueResult, ErrorKind};
pub use crate::core::message::{Message, MessageBulder};

// Traits
pub use crate::traits::backend::{BackendBuilder, QueueBackend};
pub use crate::traits::consumer::{DynConsumer, QueueConsumer};
pub use crate::traits::producer::{DynProducer, MessageId, QueueProducer};

// Features
pub use crate::feature::dlq::DlqConfig;
pub use crate::feature::shutdown::ShutdownHandle;
