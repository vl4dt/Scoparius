//! Orphaned application data detection.
//!
//! Finds config/data directories left behind by uninstalled applications.

use crate::models::AppInfo;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// Result of an orphaned data scan.
#[derive(Debug, Clone)]
pub struct OrphanedDataEntry {
    /// The orphaned directory path.
    pub path: PathBuf,
    /// Estimated size in bytes.
    pub size: u64,
    /// The inferred application name (from directory name).
    pub app_name: String,
    /// Confidence level (0.0–1.0) that this is truly orphaned.
    pub confidence: f64,
}

/// Scan for orphaned application data directories.
///
/// Compares data directories against installed application names to find
/// directories that likely belong to uninstalled applications.
pub fn find_orphaned_data(
    installed_apps: &[AppInfo],
    data_dirs: &[PathBuf],
) -> Vec<OrphanedDataEntry> {
    // Build a set of installed app name tokens for matching
    let app_tokens: HashSet<String> = installed_apps
        .iter()
        .flat_map(|app| tokenize_app_name(&app.name))
        .collect();

    let mut orphans = Vec::new();

    for base_dir in data_dirs {
        if !base_dir.exists() {
            continue;
        }

        let entries = match std::fs::read_dir(base_dir) {
            Ok(e) => e,
            Err(_) => continue,
        };

        for entry in entries.filter_map(|e| e.ok()) {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }

            let dir_name = match path.file_name() {
                Some(n) => n.to_string_lossy().to_string(),
                None => continue,
            };

            // Skip system/common directories
            if is_system_dir(&dir_name) {
                continue;
            }

            // Check if this directory matches any installed app
            let dir_tokens = tokenize_app_name(&dir_name);
            let has_match = dir_tokens.iter().any(|t| app_tokens.contains(t));

            if !has_match {
                let size = dir_size_quick(&path);
                let confidence = compute_confidence(&dir_name, size);

                if confidence > 0.3 && size > 0 {
                    orphans.push(OrphanedDataEntry {
                        path,
                        size,
                        app_name: dir_name,
                        confidence,
                    });
                }
            }
        }
    }

    // Sort by size descending
    orphans.sort_by(|a, b| b.size.cmp(&a.size));
    orphans
}

/// Get standard application data directories to scan for orphans.
pub fn app_data_directories() -> Vec<PathBuf> {
    let mut dirs = Vec::new();

    #[cfg(target_os = "windows")]
    {
        if let Ok(appdata) = std::env::var("APPDATA") {
            dirs.push(PathBuf::from(&appdata));
        }
        if let Ok(local) = std::env::var("LOCALAPPDATA") {
            dirs.push(PathBuf::from(&local));
        }
        if let Ok(pd) = std::env::var("ProgramData") {
            dirs.push(PathBuf::from(pd));
        }
    }

    #[cfg(target_os = "linux")]
    {
        if let Some(home) = dirs::home_dir() {
            dirs.push(home.join(".config"));
            dirs.push(home.join(".local").join("share"));
        }
    }

    #[cfg(target_os = "macos")]
    {
        if let Some(home) = dirs::home_dir() {
            dirs.push(home.join("Library").join("Application Support"));
            dirs.push(home.join("Library").join("Preferences"));
        }
    }

    dirs
}

/// Tokenize an app name into lowercase searchable tokens.
fn tokenize_app_name(name: &str) -> Vec<String> {
    let cleaned = name
        .replace(|c: char| !c.is_alphanumeric() && c != ' ', " ")
        .to_lowercase();

    cleaned
        .split_whitespace()
        .filter(|t| t.len() >= 3) // Skip very short tokens
        .map(|t| t.to_string())
        .collect()
}

/// Check if a directory name is a known system/common directory.
fn is_system_dir(name: &str) -> bool {
    let lower = name.to_lowercase();
    matches!(
        lower.as_str(),
        "microsoft" | "windows" | "default" | "system"
            | "temp" | "cache" | "logs" | "fonts"
            | "packages" | "extensions" | "themes"
            | ".cache" | ".config" | ".local"
    )
}

/// Quick directory size estimate (non-recursive, first level only for speed).
fn dir_size_quick(path: &Path) -> u64 {
    let mut total = 0u64;
    if let Ok(entries) = std::fs::read_dir(path) {
        for entry in entries.filter_map(|e| e.ok()) {
            if let Ok(meta) = entry.metadata() {
                if meta.is_file() {
                    total += meta.len();
                } else if meta.is_dir() {
                    // Add a rough estimate for subdirectories
                    total += dir_size_recursive(&entry.path(), 2);
                }
            }
        }
    }
    total
}

/// Recursive directory size with depth limit.
fn dir_size_recursive(path: &Path, max_depth: usize) -> u64 {
    if max_depth == 0 {
        return 0;
    }
    let mut total = 0u64;
    if let Ok(entries) = std::fs::read_dir(path) {
        for entry in entries.filter_map(|e| e.ok()) {
            if let Ok(meta) = entry.metadata() {
                if meta.is_file() {
                    total += meta.len();
                } else if meta.is_dir() {
                    total += dir_size_recursive(&entry.path(), max_depth - 1);
                }
            }
        }
    }
    total
}

/// Compute confidence that a directory is orphaned (0.0–1.0).
fn compute_confidence(dir_name: &str, size: u64) -> f64 {
    let mut confidence: f64 = 0.5;

    // Directories with common app data patterns get higher confidence
    let lower = dir_name.to_lowercase();
    if lower.contains("studio") || lower.contains("app") || lower.contains("software") {
        confidence += 0.1;
    }

    // Very small dirs are less likely interesting
    if size < 1024 {
        confidence -= 0.2;
    } else if size > 10 * 1024 * 1024 {
        confidence += 0.1; // Larger = more likely a real app's data
    }

    // Directories that look like reverse-domain (com.example.app) are likely app data
    if dir_name.contains('.') && dir_name.chars().filter(|c| *c == '.').count() >= 2 {
        confidence += 0.2;
    }

    confidence.clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tokenize_app_name() {
        let tokens = tokenize_app_name("Visual Studio Code");
        assert!(tokens.contains(&"visual".to_string()));
        assert!(tokens.contains(&"studio".to_string()));
        assert!(tokens.contains(&"code".to_string()));
    }

    #[test]
    fn test_system_dir_detection() {
        assert!(is_system_dir("Microsoft"));
        assert!(is_system_dir("windows"));
        assert!(!is_system_dir("MyCustomApp"));
    }

    #[test]
    fn test_confidence_scoring() {
        let c1 = compute_confidence("com.example.app", 5_000_000);
        let c2 = compute_confidence("randomdir", 100);
        assert!(c1 > c2);
    }

    #[test]
    fn test_find_orphaned_empty() {
        let apps: Vec<AppInfo> = Vec::new();
        let dirs: Vec<PathBuf> = Vec::new();
        let orphans = find_orphaned_data(&apps, &dirs);
        assert!(orphans.is_empty());
    }
}
