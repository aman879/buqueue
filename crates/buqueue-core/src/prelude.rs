//! Common imports for buqueue-core
//!
//! Instead of importing multiple modules, you can do:
//! 
//! ```rust
//! use buqueue_core::prelude::*;
//! ```

// Core types
pub use crate::core::message::{Message, MessageBulder};
pub use crate::core::delivery::Delivery;
pub use crate::core::error::{BuqueueError, BuqueueResult, ErrorKind};

// Traits
pub use crate::traits::producer::{QueueProducer, DynProducer, MessageId};
pub use crate::traits::consumer::{QueueConsumer, DynConsumer};
pub use crate::traits::backend::{QueueBackend, BackendBuilder};

// Features
pub use crate::feature::dlq::DlqConfig;
pub use crate::feature::shutdown::ShutdownHandle;