//! # buqueue-core
//!
//! The foundatinal traits and types for the buqueue message queue abstraction
//!
//! This create defines the contract that every backend must implement
//! It has **zero backend dependencies**
//!
//! ## Rust 2024 edition
//!
//! All traits use **native async function in traits** (AFIT)
//! The `async-trait` proc-macro create is not used anywhere in buqueue
//!
//! ## Module structure
//!
//! - [`core`]: fundamental types: `Message`, `Delivery`, errors
//! - [`traits`]: abstractions backends implement: `QueueProducer`, `QueueConsumer`, `QueueBackend`
//! - [`feature`]: opt-in behaviours: `DlqConfig`, `ShutdownHandle`
//! - [`prelude`]: everything a typical user needs in one glob import
//!
//! ## For backend implementors
//!
//! If you are writing a third-party backend for buqueue, depend only in this crate:
//!
//! ```toml
//! [dependencies]
//! buqueue-core = "0.1"
//! ```
//!
//! Then implement `QueueProducer`, `QueueConsumer` and `QueueBackend` for your backend types

#![warn(missing_docs)]
#![forbid(unsafe_code)]

pub mod core;
pub mod feature;
pub mod prelude;
pub mod traits;

pub use core::delivery;
pub use core::error;
pub use core::message;
pub use feature::dlq;
pub use feature::shutdown;
pub use traits::backend;
pub use traits::consumer;
pub use traits::producer;
