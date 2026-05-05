//! # Scoparius Core
//!
//! The core engine for the Scoparius disk analyzer and cleanup utility.
//! This crate provides all scanning, analysis, cleaning, and rule evaluation
//! logic as a platform-agnostic library consumed by the CLI and GUI frontends.

pub mod models;
pub mod platform;
pub mod scanner;
pub mod analyzer;
pub mod cleaner;
pub mod rules;
pub mod config;
pub mod error;

// Re-export key types for convenience
pub use error::{ScopariusError, Result};
pub use models::*;
pub use config::ScopariusConfig;
