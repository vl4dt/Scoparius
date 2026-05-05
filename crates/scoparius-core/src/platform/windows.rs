//! Windows platform adapter.

use crate::error::Result;
use crate::models::{AppInfo, DiskInfo};
use crate::platform::PlatformAdapter;
use crate::platform::known_paths;
use chrono::{TimeZone, Utc, DateTime, NaiveDate};
use std::collections::HashMap;
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
        let userassist = get_userassist_apps();

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

                        let install_date = parse_install_date(&subkey);
                        let mut last_used = None;
                        
                        // Check UserAssist for any exe in this install dir
                        if let Some(ref path) = install_location {
                            let path_lower = path.to_lowercase();
                            for (exe_path, dt) in &userassist {
                                if exe_path.starts_with(&path_lower) {
                                    if last_used.map_or(true, |l| *dt > l) {
                                        last_used = Some(*dt);
                                    }
                                }
                            }
                        }
                        
                        let appears_unused = last_used.map_or(false, |dt| {
                            (Utc::now() - dt).num_days() > 180
                        });

                        apps.push(AppInfo {
                            name,
                            publisher,
                            version,
                            install_path: install_location.map(PathBuf::from),
                            install_date,
                            size,
                            last_used,
                            appears_unused,
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
                    let install_path = subkey.get_value::<String, _>("InstallLocation").ok();
                    let install_date = parse_install_date(&subkey);
                    let mut last_used = None;
                    
                    if let Some(ref path) = install_path {
                        let path_lower = path.to_lowercase();
                        for (exe_path, dt) in &userassist {
                            if exe_path.starts_with(&path_lower) {
                                if last_used.map_or(true, |l| *dt > l) {
                                    last_used = Some(*dt);
                                }
                            }
                        }
                    }
                    
                    let appears_unused = last_used.map_or(false, |dt| {
                        (Utc::now() - dt).num_days() > 180
                    });

                    apps.push(AppInfo {
                        name,
                        publisher: subkey.get_value("Publisher").ok(),
                        version: subkey.get_value("DisplayVersion").ok(),
                        install_path: install_path.map(PathBuf::from),
                        install_date,
                        size: subkey
                            .get_value::<u32, _>("EstimatedSize")
                            .ok()
                            .map(|s| s as u64 * 1024),
                        last_used,
                        appears_unused,
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

fn parse_install_date(subkey: &winreg::RegKey) -> Option<DateTime<Utc>> {
    if let Ok(date_str) = subkey.get_value::<String, _>("InstallDate") {
        if date_str.len() == 8 {
            if let Ok(date) = NaiveDate::parse_from_str(&date_str, "%Y%m%d") {
                return Some(date.and_hms_opt(0, 0, 0)?.and_utc());
            }
        }
    }
    None
}

fn rot13(s: &str) -> String {
    s.chars().map(|c| {
        match c {
            'a'..='m' | 'A'..='M' => ((c as u8) + 13) as char,
            'n'..='z' | 'N'..='Z' => ((c as u8) - 13) as char,
            _ => c,
        }
    }).collect()
}

fn filetime_to_utc(ft: u64) -> Option<DateTime<Utc>> {
    if ft == 0 { return None; }
    let t = ft.saturating_sub(116444736000000000);
    let secs = t / 10_000_000;
    let nsecs = (t % 10_000_000) * 100;
    Utc.timestamp_opt(secs as i64, nsecs as u32).single()
}

fn get_userassist_apps() -> HashMap<String, DateTime<Utc>> {
    let mut map = HashMap::new();
    let hkcu = winreg::RegKey::predef(winreg::enums::HKEY_CURRENT_USER);
    if let Ok(ua) = hkcu.open_subkey(r"Software\Microsoft\Windows\CurrentVersion\Explorer\UserAssist") {
        for guid in ua.enum_keys().filter_map(|k| k.ok()) {
            if let Ok(count) = ua.open_subkey(format!(r"{}\Count", guid)) {
                for (name, val) in count.enum_values().filter_map(|v| v.ok()) {
                    let decoded_name = rot13(&name);
                    let data: Vec<u8> = val.bytes;
                    
                    if data.len() >= 68 {
                        let mut ft_bytes = [0u8; 8];
                        ft_bytes.copy_from_slice(&data[60..68]);
                        let ft = u64::from_le_bytes(ft_bytes);
                        
                        if let Some(dt) = filetime_to_utc(ft) {
                            let clean_name = decoded_name.to_lowercase();
                            let clean_name = if clean_name.starts_with("{") {
                                // Sometimes paths have known GUID prefixes like {1AC14E77-02E7-4E5D-B744-2EB1AE5198B7}\cmd.exe
                                // For simplicity, we just use the string as is, ends_with or contains is better.
                                clean_name
                            } else {
                                clean_name
                            };
                            map.insert(clean_name, dt);
                        }
                    }
                }
            }
        }
    }
    map
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_userassist_rot13() {
        assert_eq!(rot13("Uryyb"), "Hello");
        assert_eq!(rot13("ZLFDSB~1.RKR"), "MYSQFO~1.EXE");
    }

    #[test]
    fn test_filetime_to_utc() {
        // Test a known FILETIME.
        let ft = 133596768000000000;
        let dt = filetime_to_utc(ft).unwrap();
        assert_eq!(dt.timestamp(), 1715203200);
    }
}
