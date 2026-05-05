use crate::models::{FileEntry, FileCategory, RiskLevel, CleanOperation};
use std::path::PathBuf;

/// Find orphaned registry keys (e.g. uninstalled programs leaving their Uninstall keys behind).
pub fn find_orphaned_keys() -> Vec<FileEntry> {
    let mut entries = Vec::new();

    #[cfg(windows)]
    {
        use winreg::RegKey;
        use winreg::enums::{HKEY_LOCAL_MACHINE, HKEY_CURRENT_USER};

        let paths = [
            (HKEY_LOCAL_MACHINE, r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall"),
            (HKEY_LOCAL_MACHINE, r"SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall"),
            (HKEY_CURRENT_USER, r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall"),
        ];

        for (hkey, path) in paths {
            let root = format!(
                "{}\\{}",
                if hkey == HKEY_LOCAL_MACHINE { "HKLM" } else { "HKCU" },
                path
            );
            if let Ok(key) = RegKey::predef(hkey).open_subkey(path) {
                for subkey_name in key.enum_keys().filter_map(|k| k.ok()) {
                    if let Ok(subkey) = key.open_subkey(&subkey_name) {
                        let _name: String = subkey.get_value("DisplayName").unwrap_or_default();
                        let install_location: Result<String, _> = subkey.get_value("InstallLocation");
                        let uninstall_string: Result<String, _> = subkey.get_value("UninstallString");

                        // We only target keys that have an explicit InstallLocation that doesn't exist
                        // or an UninstallString that points to a non-existent file.
                        let mut orphaned = false;

                        if let Ok(loc) = install_location {
                            if !loc.trim().is_empty() {
                                let p = PathBuf::from(&loc);
                                if !p.exists() {
                                    orphaned = true;
                                }
                            }
                        }

                        if !orphaned {
                            if let Ok(uninst) = uninstall_string {
                                // Extract the exe path (often wrapped in quotes or arguments)
                                let exe = extract_exe_path(&uninst);
                                if !exe.trim().is_empty() {
                                    let p = PathBuf::from(&exe);
                                    if !p.exists() {
                                        orphaned = true;
                                    }
                                }
                            }
                        }

                        if orphaned {
                            let key_path = format!("{}\\{}", root, subkey_name);
                            entries.push(FileEntry {
                                path: PathBuf::from(key_path), // Stored as a PathBuf but used as a string for registry operations
                                size: 0,
                                is_dir: false,
                                modified: None,
                                accessed: None,
                                created: None,
                                content_hash: None,
                                category: Some(FileCategory::Registry),
                                risk: RiskLevel::High,
                            });
                        }
                    }
                }
            }
        }
    }

    entries
}

/// Helper to extract executable path from UninstallString (e.g. `"C:\App\unins000.exe" /SILENT`)
fn extract_exe_path(s: &str) -> String {
    let s = s.trim();
    if s.starts_with('"') {
        if let Some(end) = s[1..].find('"') {
            return s[1..=end].to_string();
        }
    }
    // No quotes, split by space (naive, but Windows paths usually have quotes if they contain spaces)
    s.split(' ').next().unwrap_or("").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_exe_path() {
        assert_eq!(extract_exe_path(r#""C:\Program Files\App\unins000.exe" /SILENT"#), r#"C:\Program Files\App\unins000.exe"#);
        assert_eq!(extract_exe_path(r#"C:\App\uninstall.exe -force"#), r#"C:\App\uninstall.exe"#);
        assert_eq!(extract_exe_path(r#""C:\NoArgs.exe""#), r#"C:\NoArgs.exe"#);
    }
}
