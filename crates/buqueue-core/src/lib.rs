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
//! ## What lives here
//!
//! - `Message`: the type you send into a queue
//! - `Delivery`: the type you receive from a queue
//! - `QueueProducer` - the trait for sending messages
//! - `QueueConsumer` - the trait for receiving messages
//! - `QueueBackend` - the trait that ties a backend's config to its producer and consumer types
//! - `DlqConfig` - dead letter queue configuration
//! - `BuqueueError` and `ErrorKind` - structured error types
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

#![warn(unsafe_code)]

pub mod error;
pub mod message;
pub mod delivery;
