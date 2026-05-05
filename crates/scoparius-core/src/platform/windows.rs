//! Windows platform adapter.

use crate::error::Result;
use crate::models::{AppInfo, DiskInfo};
use crate::platform::PlatformAdapter;
use crate::platform::known_paths;
use std::path::{Path, PathBuf};

/// Windows-specific platform adapter.
pub struct WindowsAdapter;

impl WindowsAdapter {
    pub fn new() -> Self {
        Self
    }
}

impl PlatformAdapter for WindowsAdapter {
    fn os_name(&self) -> &str {
        "windows"
    }

    fn list_disks(&self) -> Result<Vec<DiskInfo>> {
        use sysinfo::Disks;
        let disks = Disks::new_with_refreshed_list();
        let result = disks
            .iter()
            .map(|d| DiskInfo {
                mount_point: d.mount_point().to_path_buf(),
                name: d.name().to_string_lossy().to_string(),
                fs_type: d.file_system().to_string_lossy().to_string(),
                total_bytes: d.total_space(),
                available_bytes: d.available_space(),
                is_removable: d.is_removable(),
            })
            .collect();
        Ok(result)
    }

    fn list_installed_apps(&self) -> Result<Vec<AppInfo>> {
        let mut apps = Vec::new();

        // Query the Windows Uninstall registry keys
        let hklm_paths = [
            r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall",
            r"SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall",
        ];

        for reg_path in &hklm_paths {
            if let Ok(key) = winreg::RegKey::predef(winreg::enums::HKEY_LOCAL_MACHINE)
                .open_subkey(reg_path)
            {
                for subkey_name in key.enum_keys().filter_map(|k| k.ok()) {
                    if let Ok(subkey) = key.open_subkey(&subkey_name) {
                        let name: String = subkey
                            .get_value("DisplayName")
                            .unwrap_or_default();
                        if name.is_empty() {
                            continue;
                        }

                        let publisher: Option<String> = subkey.get_value("Publisher").ok();
                        let version: Option<String> = subkey.get_value("DisplayVersion").ok();
                        let install_location: Option<String> =
                            subkey.get_value("InstallLocation").ok();
                        let size: Option<u64> = subkey
                            .get_value::<u32, _>("EstimatedSize")
                            .ok()
                            .map(|s| s as u64 * 1024); // KB to bytes

                        apps.push(AppInfo {
                            name,
                            publisher,
                            version,
                            install_path: install_location.map(PathBuf::from),
                            install_date: None, // TODO: parse InstallDate
                            size,
                            last_used: None,
                            appears_unused: false,
                            platform_id: Some(subkey_name),
                            data_dirs: Vec::new(),
                        });
                    }
                }
            }
        }

        // Also check current user
        if let Ok(key) = winreg::RegKey::predef(winreg::enums::HKEY_CURRENT_USER)
            .open_subkey(r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall")
        {
            for subkey_name in key.enum_keys().filter_map(|k| k.ok()) {
                if let Ok(subkey) = key.open_subkey(&subkey_name) {
                    let name: String = subkey.get_value("DisplayName").unwrap_or_default();
                    if name.is_empty() || apps.iter().any(|a| a.name == name) {
                        continue;
                    }
                    apps.push(AppInfo {
                        name,
                        publisher: subkey.get_value("Publisher").ok(),
                        version: subkey.get_value("DisplayVersion").ok(),
                        install_path: subkey
                            .get_value::<String, _>("InstallLocation")
                            .ok()
                            .map(PathBuf::from),
                        install_date: None,
                        size: subkey
                            .get_value::<u32, _>("EstimatedSize")
                            .ok()
                            .map(|s| s as u64 * 1024),
                        last_used: None,
                        appears_unused: false,
                        platform_id: Some(subkey_name),
                        data_dirs: Vec::new(),
                    });
                }
            }
        }

        Ok(apps)
    }

    fn temp_dirs(&self) -> Vec<PathBuf> {
        let mut dirs = known_paths::common_temp_dirs();

        if let Ok(windir) = std::env::var("WINDIR") {
            dirs.push(PathBuf::from(&windir).join("Temp"));
            dirs.push(PathBuf::from(&windir).join("Prefetch"));
        }

        dirs
    }

    fn cache_dirs(&self) -> Vec<PathBuf> {
        let mut dirs = known_paths::common_cache_dirs();

        if let Ok(local) = std::env::var("LOCALAPPDATA") {
            dirs.push(PathBuf::from(&local).join("Temp"));
            dirs.push(PathBuf::from(&local).join("Microsoft").join("Windows").join("INetCache"));
        }

        dirs
    }

    fn log_dirs(&self) -> Vec<PathBuf> {
        let mut dirs = Vec::new();

        if let Ok(windir) = std::env::var("WINDIR") {
            dirs.push(PathBuf::from(&windir).join("Logs"));
        }
        if let Ok(pd) = std::env::var("ProgramData") {
            dirs.push(PathBuf::from(&pd).join("Microsoft").join("Windows").join("WER"));
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
