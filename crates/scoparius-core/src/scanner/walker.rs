//! Parallel filesystem walker built on `jwalk`.

use crate::error::{Result, ScopariusError};
use crate::models::{FileEntry, ProgressEvent, RiskLevel, ScanResult};
use crate::platform::known_paths;
use chrono::{DateTime, Utc};
use jwalk::WalkDir;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Instant, SystemTime};
use tokio::sync::mpsc;

/// Configuration for a scan operation.
#[derive(Debug, Clone)]
pub struct ScanConfig {
    /// Root directories to scan.
    pub roots: Vec<PathBuf>,
    /// Maximum directory depth (0 = unlimited).
    pub max_depth: usize,
    /// Follow symbolic links.
    pub follow_symlinks: bool,
    /// Skip hidden files/directories.
    pub skip_hidden: bool,
    /// Minimum file size to include (bytes). 0 = include all.
    pub min_size: u64,
    /// Number of parallel threads for walking.
    pub parallelism: usize,
    /// Paths/patterns to exclude.
    pub exclude_patterns: Vec<String>,
    /// Enable content hashing during scan.
    pub enable_hashing: bool,
}

impl Default for ScanConfig {
    fn default() -> Self {
        Self {
            roots: Vec::new(),
            max_depth: 0,
            follow_symlinks: false,
            skip_hidden: false,
            min_size: 0,
            parallelism: num_cpus(),
            exclude_patterns: Vec::new(),
            enable_hashing: false,
        }
    }
}

/// The main scanner engine.
pub struct Scanner {
    config: ScanConfig,
    protected_paths: Vec<PathBuf>,
    cancel_flag: Arc<AtomicBool>,
}

impl Scanner {
    /// Create a new scanner with the given config.
    pub fn new(config: ScanConfig) -> Self {
        Self {
            config,
            protected_paths: known_paths::system_protected_paths(),
            cancel_flag: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Get a handle to cancel the scan.
    pub fn cancel_handle(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.cancel_flag)
    }

    /// Run the scan, emitting progress events on the provided channel.
    pub async fn scan(
        &self,
        progress_tx: Option<mpsc::UnboundedSender<ProgressEvent>>,
    ) -> Result<ScanResult> {
        let start = Instant::now();
        let mut result = ScanResult::new(self.config.roots.clone());

        let files_count = Arc::new(AtomicU64::new(0));
        let dirs_count = Arc::new(AtomicU64::new(0));
        let bytes_count = Arc::new(AtomicU64::new(0));
        let error_count = Arc::new(AtomicU64::new(0));

        for root in &self.config.roots {
            if !root.exists() {
                return Err(ScopariusError::PathNotFound(root.clone()));
            }

            if let Some(tx) = &progress_tx {
                let _ = tx.send(ProgressEvent::ScanStarted {
                    root: root.clone(),
                });
            }

            let entries = self.walk_root(
                root,
                &files_count,
                &dirs_count,
                &bytes_count,
                &error_count,
                &progress_tx,
            )?;

            result.entries.extend(entries);
        }

        // Pass 2: Optional Content Hashing
        if self.config.enable_hashing {
            self.perform_hashing(&mut result, &progress_tx).await?;
        }

        result.total_files = files_count.load(Ordering::Relaxed);
        result.total_dirs = dirs_count.load(Ordering::Relaxed);
        result.total_size = bytes_count.load(Ordering::Relaxed);
        result.error_count = error_count.load(Ordering::Relaxed);
        result.completed_at = Some(Utc::now());

        let duration_ms = start.elapsed().as_millis() as u64;

        if let Some(tx) = &progress_tx {
            let _ = tx.send(ProgressEvent::ScanCompleted {
                result: result.clone(),
                duration_ms,
            });
        }

        tracing::info!(
            "Scan completed: {} files, {} dirs, {} bytes in {}ms",
            result.total_files,
            result.total_dirs,
            result.total_size,
            duration_ms,
        );

        Ok(result)
    }

    /// Walk a single root directory.
    fn walk_root(
        &self,
        root: &Path,
        files_count: &Arc<AtomicU64>,
        dirs_count: &Arc<AtomicU64>,
        bytes_count: &Arc<AtomicU64>,
        error_count: &Arc<AtomicU64>,
        progress_tx: &Option<mpsc::UnboundedSender<ProgressEvent>>,
    ) -> Result<Vec<FileEntry>> {
        let mut entries = Vec::new();
        let mut progress_counter = 0u64;

        let mut walk = WalkDir::new(root)
            .follow_links(self.config.follow_symlinks)
            .parallelism(jwalk::Parallelism::RayonNewPool(self.config.parallelism));

        if self.config.max_depth > 0 {
            walk = walk.max_depth(self.config.max_depth);
        }

        if self.config.skip_hidden {
            walk = walk.skip_hidden(true);
        }

        for entry_result in walk {
            // Check for cancellation
            if self.cancel_flag.load(Ordering::Relaxed) {
                tracing::info!("Scan cancelled by user");
                return Err(ScopariusError::Cancelled);
            }

            match entry_result {
                Ok(dir_entry) => {
                    let path = dir_entry.path();

                    // Skip protected paths
                    if self.is_protected(&path) {
                        continue;
                    }

                    // Skip excluded patterns
                    if self.is_excluded(&path) {
                        continue;
                    }

                    let metadata = match dir_entry.metadata() {
                        Ok(m) => m,
                        Err(e) => {
                            error_count.fetch_add(1, Ordering::Relaxed);
                            tracing::debug!("Cannot read metadata for {}: {}", path.display(), e);
                            continue;
                        }
                    };

                    let is_dir = metadata.is_dir();
                    let size = if is_dir { 0 } else { metadata.len() };

                    // Skip files below minimum size
                    if !is_dir && size < self.config.min_size {
                        continue;
                    }

                    if is_dir {
                        dirs_count.fetch_add(1, Ordering::Relaxed);
                    } else {
                        files_count.fetch_add(1, Ordering::Relaxed);
                        bytes_count.fetch_add(size, Ordering::Relaxed);
                    }

                    let entry = FileEntry {
                        path: path.clone(),
                        size,
                        is_dir,
                        modified: system_time_to_datetime(metadata.modified().ok()),
                        accessed: system_time_to_datetime(metadata.accessed().ok()),
                        created: system_time_to_datetime(metadata.created().ok()),
                        content_hash: None,
                        category: None,
                        risk: RiskLevel::Safe,
                    };

                    entries.push(entry);

                    // Send periodic progress events (every 500 entries)
                    progress_counter += 1;
                    if progress_counter % 500 == 0 {
                        if let Some(tx) = progress_tx {
                            let _ = tx.send(ProgressEvent::ScanProgress {
                                files_scanned: files_count.load(Ordering::Relaxed),
                                dirs_scanned: dirs_count.load(Ordering::Relaxed),
                                bytes_scanned: bytes_count.load(Ordering::Relaxed),
                                current_path: path,
                            });
                        }
                    }
                }
                Err(e) => {
                    error_count.fetch_add(1, Ordering::Relaxed);
                    tracing::debug!("Walk error: {}", e);
                }
            }
        }

        Ok(entries)
    }

    /// Check if a path is in the protected list.
    fn is_protected(&self, path: &Path) -> bool {
        self.protected_paths
            .iter()
            .any(|protected| path.starts_with(protected))
    }

    /// Check if a path matches any exclusion pattern.
    fn is_excluded(&self, path: &Path) -> bool {
        let path_str = path.to_string_lossy();
        self.config
            .exclude_patterns
            .iter()
            .any(|pattern| path_str.contains(pattern.as_str()))
    }

    /// Perform a hashing pass on candidate duplicates (same size).
    async fn perform_hashing(
        &self,
        result: &mut ScanResult,
        progress_tx: &Option<mpsc::UnboundedSender<ProgressEvent>>,
    ) -> Result<()> {
        use crate::analyzer::duplicates;
        use std::collections::HashMap;

        // Group by size to find hashing candidates (only groups with 2+ files)
        let mut size_groups: HashMap<u64, Vec<usize>> = HashMap::new();
        for (idx, entry) in result.entries.iter().enumerate() {
            if !entry.is_dir && entry.size > 0 {
                size_groups.entry(entry.size).or_default().push(idx);
            }
        }

        let candidates: Vec<usize> = size_groups
            .into_iter()
            .filter(|(_, indices)| indices.len() >= 2)
            .flat_map(|(_, indices)| indices)
            .collect();

        let total_to_hash = candidates.len() as u64;
        let mut files_hashed = 0u64;

        for &idx in &candidates {
            if self.cancel_flag.load(Ordering::Relaxed) {
                break;
            }

            let entry = &mut result.entries[idx];
            let _ = duplicates::hash_entry(entry);
            
            files_hashed += 1;
            if files_hashed % 10 == 0 || files_hashed == total_to_hash {
                if let Some(tx) = progress_tx {
                    let _ = tx.send(ProgressEvent::HashProgress {
                        files_hashed,
                        total_to_hash,
                        current_path: entry.path.clone(),
                    });
                }
            }
        }

        Ok(())
    }
}

/// Convert `SystemTime` to `DateTime<Utc>`.
fn system_time_to_datetime(time: Option<SystemTime>) -> Option<DateTime<Utc>> {
    time.map(|t| DateTime::from(t))
}

/// Get a reasonable thread count for parallelism.
fn num_cpus() -> usize {
    std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_scan_empty_directory() {
        let tmp = TempDir::new().unwrap();
        let config = ScanConfig {
            roots: vec![tmp.path().to_path_buf()],
            ..Default::default()
        };

        let scanner = Scanner::new(config);
        let result = scanner.scan(None).await.unwrap();

        assert_eq!(result.total_files, 0);
        assert!(result.completed_at.is_some());
    }

    #[tokio::test]
    async fn test_scan_with_files() {
        let tmp = TempDir::new().unwrap();

        // Create test files
        std::fs::write(tmp.path().join("file1.txt"), "hello").unwrap();
        std::fs::write(tmp.path().join("file2.log"), "world").unwrap();
        std::fs::create_dir(tmp.path().join("subdir")).unwrap();
        std::fs::write(tmp.path().join("subdir").join("nested.tmp"), "data").unwrap();

        let config = ScanConfig {
            roots: vec![tmp.path().to_path_buf()],
            ..Default::default()
        };

        let scanner = Scanner::new(config);
        let result = scanner.scan(None).await.unwrap();

        assert_eq!(result.total_files, 3);
        assert!(result.total_dirs >= 1); // subdir (root may or may not be counted)
        assert!(result.total_size > 0);
    }

    #[tokio::test]
    async fn test_scan_nonexistent_path() {
        let config = ScanConfig {
            roots: vec![PathBuf::from("/this/path/does/not/exist")],
            ..Default::default()
        };

        let scanner = Scanner::new(config);
        let result = scanner.scan(None).await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_scan_min_size_filter() {
        let tmp = TempDir::new().unwrap();

        std::fs::write(tmp.path().join("small.txt"), "hi").unwrap(); // 2 bytes
        std::fs::write(tmp.path().join("big.txt"), "a".repeat(1000)).unwrap(); // 1000 bytes

        let config = ScanConfig {
            roots: vec![tmp.path().to_path_buf()],
            min_size: 100,
            ..Default::default()
        };

        let scanner = Scanner::new(config);
        let result = scanner.scan(None).await.unwrap();

        // Only the big file should be included
        assert_eq!(result.total_files, 1);
    }

    #[tokio::test]
    async fn test_scan_cancellation() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("file.txt"), "data").unwrap();

        let config = ScanConfig {
            roots: vec![tmp.path().to_path_buf()],
            ..Default::default()
        };

        let scanner = Scanner::new(config);
        // Cancel immediately
        scanner.cancel_handle().store(true, Ordering::Relaxed);

        let result = scanner.scan(None).await;
        assert!(matches!(result, Err(ScopariusError::Cancelled)));
    }

    #[tokio::test]
    async fn test_progress_events() {
        let tmp = TempDir::new().unwrap();
        for i in 0..10 {
            std::fs::write(tmp.path().join(format!("file_{i}.txt")), format!("content {i}")).unwrap();
        }

        let config = ScanConfig {
            roots: vec![tmp.path().to_path_buf()],
            ..Default::default()
        };

        let (tx, mut rx) = mpsc::unbounded_channel();
        let scanner = Scanner::new(config);
        let result = scanner.scan(Some(tx)).await.unwrap();

        assert_eq!(result.total_files, 10);

        // Should have at least ScanStarted and ScanCompleted events
        let mut events = Vec::new();
        while let Ok(event) = rx.try_recv() {
            events.push(event);
        }
        assert!(events.len() >= 2);
        assert!(matches!(events.first(), Some(ProgressEvent::ScanStarted { .. })));
        assert!(matches!(events.last(), Some(ProgressEvent::ScanCompleted { .. })));
    }
}
