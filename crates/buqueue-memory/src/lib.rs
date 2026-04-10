//! # buqueue-memory
//!
//! An in-memory implentation if the `buqueue` traits
//!
//! This backend is ideal for:
//! - Unit and integration testing withouth external dependecies
//! - Local development of event-driven logic
//! - High-speed, transient messaging withing a single process
//!
//! It supports all core features: AFIT (Async Functions in Traits),
//! Graceful shutdown, Scheduled Delivery (`send_at`), and Dead Letter Queues (DLQ)
//!
//! ## Quick example
//!
//! ```rust
//! # tokio_test::block_on(async {
//! use buqueue_memory::{MemoryBackend, MemoryConfig};
//! use buqueue_core::prelude::*;
//!
//! let (producer, consumer) = MemoryBackend::builder(MemoryConfig::default())
//!     .build_par()
//!     .await
//!     .unwrap();
//!   
//! producer.send(Message::from_json(&42u32).unwrap()).await.unwrap();
//! let delivery = consumer.receive().await.unwrap();
//! assert_eq!(delivery.payload_json::<u32>().unwrap(), 42);
//! delivery.ack().await.unwrap();
//! # });
//! ```

#![warn(missing_docs)]
#![forbid(unsafe_code)]

pub mod ack;
pub mod config;
pub mod producer;
pub(crate) mod envelope;


pub use config::MemoryConfig;