//! File categorization and analysis engine.
//!
//! Categorizes scanned files into cleanup categories and assigns risk levels.

pub mod duplicates;
pub mod orphans;
pub mod cloud;
pub mod registry;

use crate::config::ThresholdConfig;
use crate::models::{FileCategory, FileEntry, RiskLevel, ScanResult};
use chrono::Utc;
use std::collections::HashMap;
use std::path::Path;

/// Analyzer that categorizes file entries from scan results.
pub struct Analyzer {
    thresholds: ThresholdConfig,
}

impl Analyzer {
    /// Create a new analyzer with the given thresholds.
    pub fn new(thresholds: ThresholdConfig) -> Self {
        Self { thresholds }
    }

    /// Analyze a scan result, categorizing all entries in-place.
    pub fn analyze(&self, result: &mut ScanResult) {
        for entry in &mut result.entries {
            if entry.is_dir {
                continue;
            }
            if let Some((category, risk)) = self.classify(entry) {
                entry.category = Some(category);
                entry.risk = risk;
            }
        }
    }

    /// Classify a single file entry.
    fn classify(&self, entry: &FileEntry) -> Option<(FileCategory, RiskLevel)> {
        let path = &entry.path;

        // Check each category in priority order

        // 1. Temporary files
        if self.is_temp_file(path, entry) {
            return Some((FileCategory::TempFile, RiskLevel::Safe));
        }

        // 2. Cache files
        if self.is_cache_file(path) {
            return Some((FileCategory::Cache, RiskLevel::Safe));
        }

        // 3. Log files
        if self.is_log_file(path, entry) {
            return Some((FileCategory::Log, RiskLevel::Low));
        }

        // 4. Crash dumps
        if self.is_crash_dump(path) {
            return Some((FileCategory::CrashDump, RiskLevel::Safe));
        }

        // 5. Thumbnails
        if self.is_thumbnail(path) {
            return Some((FileCategory::Thumbnail, RiskLevel::Safe));
        }

        // 6. Build artifacts
        if self.is_build_artifact(path, entry) {
            return Some((FileCategory::BuildArtifact, RiskLevel::Medium));
        }

        // 7. OS junk
        if self.is_os_junk(path) {
            return Some((FileCategory::OsJunk, RiskLevel::Safe));
        }

        // 8. Package manager cache
        if self.is_package_manager_cache(path) {
            return Some((FileCategory::PackageManagerCache, RiskLevel::Low));
        }

        // 9. Large & stale
        if self.is_large_and_stale(entry) {
            return Some((FileCategory::LargeStale, RiskLevel::Medium));
        }

        None
    }

    fn is_temp_file(&self, path: &Path, entry: &FileEntry) -> bool {
        let ext = extension_lower(path);
        let in_temp_dir = is_in_directory_named(path, &["temp", "tmp", "Temp", "Tmp"]);

        let is_temp_ext = matches!(
            ext.as_deref(),
            Some("tmp") | Some("temp") | Some("bak") | Some("swp") | Some("swo")
        );

        let name = filename_lower(path);
        let is_temp_name = name.as_ref().is_some_and(|n| {
            n.starts_with("~$") || n.starts_with("~") && n.ends_with(".tmp")
        });

        let stale = is_older_than_days(entry, self.thresholds.temp_days);

        (is_temp_ext || is_temp_name || in_temp_dir) && stale
    }

    fn is_cache_file(&self, path: &Path) -> bool {
        is_in_directory_named(path, &["cache", "Cache", "caches", "Caches", "cache2"])
    }

    fn is_log_file(&self, path: &Path, entry: &FileEntry) -> bool {
        let ext = extension_lower(path);
        let is_log_ext = matches!(ext.as_deref(), Some("log") | Some("log.1") | Some("log.gz"));
        let in_log_dir = is_in_directory_named(path, &["logs", "log", "Logs"]);

        (is_log_ext || in_log_dir) && is_older_than_days(entry, self.thresholds.log_days)
    }

    fn is_crash_dump(&self, path: &Path) -> bool {
        let ext = extension_lower(path);
        matches!(
            ext.as_deref(),
            Some("dmp") | Some("crash") | Some("mdmp") | Some("core")
        )
    }

    fn is_thumbnail(&self, path: &Path) -> bool {
        let name = filename_lower(path);
        name.as_ref().is_some_and(|n| {
            n == "thumbs.db" || n == ".ds_store" || n == "desktop.ini"
        }) || is_in_directory_named(path, &["thumbnails", "Thumbnails", "thumb"])
    }

    fn is_build_artifact(&self, path: &Path, entry: &FileEntry) -> bool {
        let build_dirs = [
            "target", "node_modules", "__pycache__", ".gradle",
            "build", "dist", ".next", ".nuxt", ".cache",
            "obj", "bin",
        ];
        is_in_directory_named(path, &build_dirs)
            && is_older_than_days(entry, self.thresholds.build_artifact_days)
    }

    fn is_os_junk(&self, path: &Path) -> bool {
        let name = filename_lower(path);
        name.as_ref().is_some_and(|n| {
            matches!(
                n.as_str(),
                "thumbs.db"
                    | ".ds_store"
                    | "desktop.ini"
                    | ".spotlight-v100"
                    | ".trashes"
                    | ".fseventsd"
                    | ".temporaryitems"
            )
        })
    }

    fn is_package_manager_cache(&self, path: &Path) -> bool {
        let pm_dirs = [
            ".npm", ".yarn", ".pnpm-store",
            ".cargo/registry", ".cargo/git",
            ".m2/repository", ".gradle/caches",
            ".nuget/packages", "pip/cache",
        ];
        let path_str = path.to_string_lossy();
        pm_dirs.iter().any(|d| path_str.contains(d))
    }

    fn is_large_and_stale(&self, entry: &FileEntry) -> bool {
        entry.size >= self.thresholds.large_file_bytes
            && is_older_than_days(entry, self.thresholds.stale_days)
    }

    /// Get a summary of categorized entries.
    pub fn summary(result: &ScanResult) -> HashMap<FileCategory, CategorySummary> {
        let mut map: HashMap<FileCategory, CategorySummary> = HashMap::new();

        for entry in &result.entries {
            if let Some(cat) = entry.category {
                let summary = map.entry(cat).or_insert_with(|| CategorySummary {
                    category: cat,
                    file_count: 0,
                    total_size: 0,
                });
                summary.file_count += 1;
                summary.total_size += entry.size;
            }
        }

        map
    }
}

/// Summary statistics for a file category.
#[derive(Debug, Clone)]
pub struct CategorySummary {
    pub category: FileCategory,
    pub file_count: u64,
    pub total_size: u64,
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

fn extension_lower(path: &Path) -> Option<String> {
    path.extension()
        .map(|ext| ext.to_string_lossy().to_lowercase())
}

fn filename_lower(path: &Path) -> Option<String> {
    path.file_name()
        .map(|name| name.to_string_lossy().to_lowercase())
}

fn is_in_directory_named(path: &Path, names: &[&str]) -> bool {
    path.components().any(|c| {
        if let std::path::Component::Normal(s) = c {
            let s = s.to_string_lossy();
            names.iter().any(|n| s == *n)
        } else {
            false
        }
    })
}

fn is_older_than_days(entry: &FileEntry, days: u64) -> bool {
    if days == 0 {
        return true;
    }

    // Use accessed time if available, otherwise modified time
    let reference_time = entry.accessed.or(entry.modified);

    match reference_time {
        Some(time) => {
            let age = Utc::now().signed_duration_since(time);
            age.num_days() >= days as i64
        }
        None => true, // If we can't determine age, treat as old
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;
    use std::path::PathBuf;

    fn make_entry(path: &str, size: u64, days_old: i64) -> FileEntry {
        let accessed = Some(Utc::now() - Duration::days(days_old));
        FileEntry {
            path: PathBuf::from(path),
            size,
            is_dir: false,
            modified: accessed,
            accessed,
            created: None,
            content_hash: None,
            category: None,
            risk: RiskLevel::Safe,
        }
    }

    #[test]
    fn test_classify_temp_file() {
        let analyzer = Analyzer::new(ThresholdConfig::default());
        let entry = make_entry("/tmp/foo.tmp", 100, 10);
        let result = analyzer.classify(&entry);
        assert!(matches!(result, Some((FileCategory::TempFile, _))));
    }

    #[test]
    fn test_classify_log_file() {
        let analyzer = Analyzer::new(ThresholdConfig::default());
        let entry = make_entry("/var/log/app.log", 5000, 100);
        let result = analyzer.classify(&entry);
        assert!(matches!(result, Some((FileCategory::Log, _))));
    }

    #[test]
    fn test_classify_crash_dump() {
        let analyzer = Analyzer::new(ThresholdConfig::default());
        let entry = make_entry("/home/user/crash.dmp", 1_000_000, 5);
        let result = analyzer.classify(&entry);
        assert!(matches!(result, Some((FileCategory::CrashDump, _))));
    }

    #[test]
    fn test_fresh_temp_file_not_classified() {
        let analyzer = Analyzer::new(ThresholdConfig::default());
        let entry = make_entry("/tmp/recent.tmp", 100, 1); // Only 1 day old
        let result = analyzer.classify(&entry);
        // temp_days default is 7, so 1-day-old file should NOT be classified
        assert!(result.is_none());
    }

    #[test]
    fn test_large_stale_file() {
        let analyzer = Analyzer::new(ThresholdConfig::default());
        let entry = make_entry("/home/user/bigfile.iso", 500_000_000, 200);
        let result = analyzer.classify(&entry);
        assert!(matches!(result, Some((FileCategory::LargeStale, _))));
    }
}
