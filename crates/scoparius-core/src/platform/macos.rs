//! macOS platform adapter.

use crate::error::Result;
use crate::models::{AppInfo, DiskInfo};
use crate::platform::PlatformAdapter;
use crate::platform::known_paths;
use std::path::{Path, PathBuf};

/// macOS-specific platform adapter.
pub struct MacosAdapter;

impl MacosAdapter {
    pub fn new() -> Self {
        Self
    }
}

impl PlatformAdapter for MacosAdapter {
    fn os_name(&self) -> &str {
        "macos"
    }

    fn list_disks(&self) -> Result<Vec<DiskInfo>> {
        use sysinfo::Disks;
        let disks = Disks::new_with_refreshed_list();
        Ok(disks
            .iter()
            .map(|d| DiskInfo {
                mount_point: d.mount_point().to_path_buf(),
                name: d.name().to_string_lossy().to_string(),
                fs_type: d.file_system().to_string_lossy().to_string(),
                total_bytes: d.total_space(),
                available_bytes: d.available_space(),
                is_removable: d.is_removable(),
            })
            .collect())
    }

    fn list_installed_apps(&self) -> Result<Vec<AppInfo>> {
        let mut apps = Vec::new();
        let app_dirs = [
            PathBuf::from("/Applications"),
            dirs::home_dir()
                .map(|h| h.join("Applications"))
                .unwrap_or_default(),
        ];

        for dir in &app_dirs {
            if let Ok(entries) = std::fs::read_dir(dir) {
                for entry in entries.filter_map(|e| e.ok()) {
                    let path = entry.path();
                    if path.extension().is_some_and(|ext| ext == "app") {
                        let name = path
                            .file_stem()
                            .map(|s| s.to_string_lossy().to_string())
                            .unwrap_or_default();

                        let size = dir_size(&path).ok();

                        apps.push(AppInfo {
                            name,
                            publisher: None,
                            version: None,
                            install_path: Some(path.clone()),
                            install_date: None,
                            size,
                            last_used: None,
                            appears_unused: false,
                            platform_id: Some(path.to_string_lossy().to_string()),
                            data_dirs: Vec::new(),
                        });
                    }
                }
            }
        }

        Ok(apps)
    }

    fn temp_dirs(&self) -> Vec<PathBuf> {
        known_paths::common_temp_dirs()
    }

    fn cache_dirs(&self) -> Vec<PathBuf> {
        let mut dirs = known_paths::common_cache_dirs();

        if let Some(home) = dirs::home_dir() {
            dirs.push(home.join("Library").join("Caches"));
        }

        dirs
    }

    fn log_dirs(&self) -> Vec<PathBuf> {
        let mut dirs = vec![PathBuf::from("/var/log")];

        if let Some(home) = dirs::home_dir() {
            dirs.push(home.join("Library").join("Logs"));
        }

        dirs
    }

    fn protected_paths(&self) -> Vec<PathBuf> {
        known_paths::system_protected_paths()
    }

    fn is_cloud_synced(&self, path: &Path) -> bool {
        known_paths::cloud_sync_dirs()
            .iter()
            .any(|(dir, _)| path.starts_with(dir))
    }
}

/// Calculate total directory size recursively.
fn dir_size(path: &Path) -> std::io::Result<u64> {
    let mut total = 0u64;
    if path.is_dir() {
        for entry in std::fs::read_dir(path)? {
            let entry = entry?;
            let meta = entry.metadata()?;
            if meta.is_dir() {
                total += dir_size(&entry.path()).unwrap_or(0);
            } else {
                total += meta.len();
            }
        }
    }
    Ok(total)
}
