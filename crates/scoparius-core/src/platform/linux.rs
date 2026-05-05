//! Linux platform adapter.

use crate::error::Result;
use crate::models::{AppInfo, DiskInfo};
use crate::platform::PlatformAdapter;
use crate::platform::known_paths;
use std::path::{Path, PathBuf};

/// Linux-specific platform adapter.
pub struct LinuxAdapter;

impl LinuxAdapter {
    pub fn new() -> Self {
        Self
    }
}

impl PlatformAdapter for LinuxAdapter {
    fn os_name(&self) -> &str {
        "linux"
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

        // Parse .desktop files from XDG data dirs
        let xdg_dirs = std::env::var("XDG_DATA_DIRS")
            .unwrap_or_else(|_| "/usr/share:/usr/local/share".to_string());

        for dir in xdg_dirs.split(':') {
            let apps_dir = PathBuf::from(dir).join("applications");
            if let Ok(entries) = std::fs::read_dir(&apps_dir) {
                for entry in entries.filter_map(|e| e.ok()) {
                    let path = entry.path();
                    if path.extension().is_some_and(|ext| ext == "desktop") {
                        if let Ok(contents) = std::fs::read_to_string(&path) {
                            let name = parse_desktop_field(&contents, "Name");
                            if let Some(name) = name {
                                apps.push(AppInfo {
                                    name,
                                    publisher: parse_desktop_field(&contents, "GenericName"),
                                    version: None,
                                    install_path: parse_desktop_field(&contents, "Exec")
                                        .map(|e| PathBuf::from(e.split_whitespace().next().unwrap_or(""))),
                                    install_date: None,
                                    size: None,
                                    last_used: None,
                                    appears_unused: false,
                                    platform_id: path.file_stem().map(|s| s.to_string_lossy().to_string()),
                                    data_dirs: Vec::new(),
                                });
                            }
                        }
                    }
                }
            }
        }

        Ok(apps)
    }

    fn temp_dirs(&self) -> Vec<PathBuf> {
        let mut dirs = known_paths::common_temp_dirs();
        dirs.push(PathBuf::from("/tmp"));
        dirs.push(PathBuf::from("/var/tmp"));
        dirs
    }

    fn cache_dirs(&self) -> Vec<PathBuf> {
        let mut dirs = known_paths::common_cache_dirs();

        if let Some(home) = dirs::home_dir() {
            dirs.push(home.join(".cache"));
        }

        dirs
    }

    fn log_dirs(&self) -> Vec<PathBuf> {
        vec![
            PathBuf::from("/var/log"),
        ]
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

/// Parse a field from a .desktop file.
fn parse_desktop_field(contents: &str, key: &str) -> Option<String> {
    let prefix = format!("{key}=");
    contents
        .lines()
        .find(|line| line.starts_with(&prefix))
        .map(|line| line[prefix.len()..].trim().to_string())
}
