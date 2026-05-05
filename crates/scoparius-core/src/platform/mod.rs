//! Platform abstraction layer.
//!
//! Defines traits and platform-specific implementations for OS-dependent
//! operations like app discovery, registry access, and known paths.

pub mod known_paths;

#[cfg(target_os = "windows")]
pub mod windows;
#[cfg(target_os = "linux")]
pub mod linux;
#[cfg(target_os = "macos")]
pub mod macos;

use crate::models::{AppInfo, DiskInfo};
use crate::error::Result;
use std::path::PathBuf;

/// Trait for platform-specific operations.
pub trait PlatformAdapter: Send + Sync {
    /// Get the current operating system name.
    fn os_name(&self) -> &str;

    /// List all mounted disks / volumes.
    fn list_disks(&self) -> Result<Vec<DiskInfo>>;

    /// List installed applications.
    fn list_installed_apps(&self) -> Result<Vec<AppInfo>>;

    /// Get known temporary file directories.
    fn temp_dirs(&self) -> Vec<PathBuf>;

    /// Get known cache directories.
    fn cache_dirs(&self) -> Vec<PathBuf>;

    /// Get known log directories.
    fn log_dirs(&self) -> Vec<PathBuf>;

    /// Get system-protected paths that must never be cleaned.
    fn protected_paths(&self) -> Vec<PathBuf>;

    /// Check if a given path is cloud-synced (OneDrive, Dropbox, etc.).
    fn is_cloud_synced(&self, path: &std::path::Path) -> bool;
}

/// Create the appropriate platform adapter for the current OS.
pub fn create_adapter() -> Box<dyn PlatformAdapter> {
    #[cfg(target_os = "windows")]
    { Box::new(windows::WindowsAdapter::new()) }
    #[cfg(target_os = "linux")]
    { Box::new(linux::LinuxAdapter::new()) }
    #[cfg(target_os = "macos")]
    { Box::new(macos::MacosAdapter::new()) }
}
