//! Filesystem scanner engine.
//!
//! Uses `jwalk` for parallel directory traversal with progress reporting
//! via channels.

pub mod walker;

pub use walker::{Scanner, ScanConfig};
