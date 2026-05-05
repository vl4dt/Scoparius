//! Cloud sync directory detection and conflict file finder.

use crate::models::FileEntry;
use std::path::{Path, PathBuf};

/// Information about a detected cloud sync provider.
#[derive(Debug, Clone)]
pub struct CloudSyncInfo {
    /// Provider name (e.g., "OneDrive", "Dropbox").
    pub provider: String,
    /// Root directory of the sync.
    pub root: PathBuf,
    /// Whether the directory actually exists.
    pub exists: bool,
}

/// Detect all cloud sync directories on the system.
pub fn detect_cloud_providers() -> Vec<CloudSyncInfo> {
    let home = match dirs::home_dir() {
        Some(h) => h,
        None => return Vec::new(),
    };

    let providers = [
        ("OneDrive", vec![
            home.join("OneDrive"),
            home.join("OneDrive - Personal"),
        ]),
        ("Dropbox", vec![home.join("Dropbox")]),
        ("Google Drive", vec![
            home.join("Google Drive"),
            home.join("My Drive"),
        ]),
        ("iCloud", vec![home.join("iCloudDrive")]),
        ("MEGA", vec![home.join("MEGA")]),
        ("pCloud", vec![home.join("pCloud Drive")]),
        ("Box", vec![home.join("Box")]),
    ];

    let mut detected = Vec::new();
    for (name, paths) in &providers {
        for path in paths {
            detected.push(CloudSyncInfo {
                provider: name.to_string(),
                root: path.clone(),
                exists: path.exists(),
            });
        }
    }

    // Also check Windows-specific OneDrive registry path
    #[cfg(target_os = "windows")]
    {
        if let Ok(key) = winreg::RegKey::predef(winreg::enums::HKEY_CURRENT_USER)
            .open_subkey(r"Software\Microsoft\OneDrive")
        {
            if let Ok(path) = key.get_value::<String, _>("UserFolder") {
                let p = PathBuf::from(&path);
                if p.exists() && !detected.iter().any(|d| d.root == p) {
                    detected.push(CloudSyncInfo {
                        provider: "OneDrive".into(),
                        root: p,
                        exists: true,
                    });
                }
            }
        }
    }

    detected
}

/// Check if a path is inside any cloud sync directory.
pub fn is_cloud_synced<'a>(path: &Path, providers: &'a [CloudSyncInfo]) -> Option<&'a str> {
    providers
        .iter()
        .find(|p| p.exists && path.starts_with(&p.root))
        .map(|p| p.provider.as_str())
}

/// Find cloud sync conflict files in scan entries.
///
/// Common conflict patterns:
/// - `file (conflicted copy).ext` (Dropbox)
/// - `file-Author's conflicting copy.ext` (OneDrive)  
/// - `file (1).ext` (generic duplicate)
pub fn find_conflict_files(entries: &[FileEntry]) -> Vec<&FileEntry> {
    entries
        .iter()
        .filter(|e| {
            if e.is_dir { return false; }
            let name = e.path.file_name()
                .map(|n| n.to_string_lossy().to_lowercase())
                .unwrap_or_default();

            name.contains("conflicted copy")
                || name.contains("conflicting copy")
                || name.contains("conflict")
                || (name.contains('(') && name.contains(')') && is_numbered_copy(&name))
        })
        .collect()
}

/// Check if a filename looks like a numbered copy: "file (1).ext", "file (2).ext".
fn is_numbered_copy(name: &str) -> bool {
    if let Some(start) = name.rfind('(') {
        if let Some(end) = name[start..].find(')') {
            let inner = &name[start + 1..start + end];
            return inner.trim().parse::<u32>().is_ok();
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_numbered_copy_detection() {
        assert!(is_numbered_copy("report (1).docx"));
        assert!(is_numbered_copy("photo (23).jpg"));
        assert!(!is_numbered_copy("normal_file.txt"));
        assert!(!is_numbered_copy("file (abc).txt"));
    }

    #[test]
    fn test_detect_cloud_providers_runs() {
        let providers = detect_cloud_providers();
        // Should at least generate entries (may not exist on all systems)
        assert!(!providers.is_empty());
    }
}
