//! Configuration for the in-memory backend

/// Configuration for the in-memory backend
///
/// There is intentinally very little to configure, this backend exists
/// for testing, not production.
#[derive(Debug, Clone, Default)]
pub struct MemoryConfig {
    /// Internal channel buffer capacity
    ///
    /// Default to `1024`. Increase if your tests send may messages
    /// before consuming any of them
    pub capacity: Option<usize>,
}
