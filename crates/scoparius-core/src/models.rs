//! Core data models used throughout Scoparius.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use uuid::Uuid;

// ─── File Entry ──────────────────────────────────────────────────────────────

/// A single discovered filesystem entry with metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileEntry {
    /// Absolute path to the file.
    pub path: PathBuf,
    /// File size in bytes.
    pub size: u64,
    /// Whether this is a directory.
    pub is_dir: bool,
    /// Last modification time.
    pub modified: Option<DateTime<Utc>>,
    /// Last access time (may be unreliable on some filesystems).
    pub accessed: Option<DateTime<Utc>>,
    /// Creation time (platform-dependent).
    pub created: Option<DateTime<Utc>>,
    /// Content hash (computed on-demand for duplicate detection).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_hash: Option<String>,
    /// Categorization assigned by the analyzer.
    pub category: Option<FileCategory>,
    /// Risk level for deletion.
    pub risk: RiskLevel,
}

// ─── Categories ──────────────────────────────────────────────────────────────

/// Cleanup category for a file or directory.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FileCategory {
    TempFile,
    Cache,
    Log,
    Thumbnail,
    CrashDump,
    BuildArtifact,
    Duplicate,
    LargeStale,
    OrphanedAppData,
    BrowserData,
    PackageManagerCache,
    OsJunk,
    CloudSyncConflict,
    Registry,
    Custom(u32),
}

impl FileCategory {
    /// Human-readable display name.
    pub fn display_name(&self) -> &str {
        match self {
            Self::TempFile => "Temporary Files",
            Self::Cache => "Cache",
            Self::Log => "Logs",
            Self::Thumbnail => "Thumbnails",
            Self::CrashDump => "Crash Dumps",
            Self::BuildArtifact => "Build Artifacts",
            Self::Duplicate => "Duplicates",
            Self::LargeStale => "Large & Stale",
            Self::OrphanedAppData => "Orphaned App Data",
            Self::BrowserData => "Browser Data",
            Self::PackageManagerCache => "Package Manager Cache",
            Self::OsJunk => "OS Junk Files",
            Self::CloudSyncConflict => "Cloud Sync Conflicts",
            Self::Registry => "Registry Entries",
            Self::Custom(_) => "Custom",
        }
    }

    /// Default staleness threshold in days for this category.
    pub fn default_threshold_days(&self) -> u64 {
        match self {
            Self::TempFile => 7,
            Self::Cache => 30,
            Self::Log => 90,
            Self::Thumbnail => 30,
            Self::CrashDump => 14,
            Self::BuildArtifact => 30,
            Self::Duplicate => 0,       // Always eligible
            Self::LargeStale => 180,
            Self::OrphanedAppData => 0,  // Always eligible
            Self::BrowserData => 30,
            Self::PackageManagerCache => 30,
            Self::OsJunk => 7,
            Self::CloudSyncConflict => 14,
            Self::Registry => 0,
            Self::Custom(_) => 30,
        }
    }
}

impl std::fmt::Display for FileCategory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.display_name())
    }
}

// ─── Risk Levels ─────────────────────────────────────────────────────────────

/// Risk level associated with deleting a file or performing an operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RiskLevel {
    /// Completely safe — system temp files, caches.
    Safe,
    /// Low risk — browser caches, old logs.
    Low,
    /// Medium risk — build artifacts, large stale files.
    Medium,
    /// High risk — app data, registry entries.
    High,
    /// Critical — system files (should never be auto-cleaned).
    Critical,
}

impl RiskLevel {
    /// Whether this risk level requires explicit user approval.
    pub fn requires_approval(&self) -> bool {
        matches!(self, Self::Medium | Self::High | Self::Critical)
    }
}

impl std::fmt::Display for RiskLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Safe => write!(f, "SAFE"),
            Self::Low => write!(f, "LOW"),
            Self::Medium => write!(f, "MEDIUM"),
            Self::High => write!(f, "HIGH"),
            Self::Critical => write!(f, "CRITICAL"),
        }
    }
}

// ─── Scan Result ─────────────────────────────────────────────────────────────

/// Complete result of a filesystem scan.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanResult {
    /// Unique scan identifier.
    pub id: Uuid,
    /// Root paths that were scanned.
    pub roots: Vec<PathBuf>,
    /// When the scan was started.
    pub started_at: DateTime<Utc>,
    /// When the scan completed.
    pub completed_at: Option<DateTime<Utc>>,
    /// All discovered file entries.
    pub entries: Vec<FileEntry>,
    /// Total size of all scanned files (bytes).
    pub total_size: u64,
    /// Total number of files scanned.
    pub total_files: u64,
    /// Total number of directories scanned.
    pub total_dirs: u64,
    /// Number of errors encountered during scan.
    pub error_count: u64,
    /// Paths that could not be accessed.
    pub inaccessible_paths: Vec<PathBuf>,
}

impl ScanResult {
    /// Create a new empty scan result.
    pub fn new(roots: Vec<PathBuf>) -> Self {
        Self {
            id: Uuid::new_v4(),
            roots,
            started_at: Utc::now(),
            completed_at: None,
            entries: Vec::new(),
            total_size: 0,
            total_files: 0,
            total_dirs: 0,
            error_count: 0,
            inaccessible_paths: Vec::new(),
        }
    }

    /// Total reclaimable space (sum of all categorized entries).
    pub fn reclaimable_size(&self) -> u64 {
        self.entries
            .iter()
            .filter(|e| e.category.is_some())
            .map(|e| e.size)
            .sum()
    }

    /// Get entries grouped by category.
    pub fn by_category(&self) -> std::collections::HashMap<FileCategory, Vec<&FileEntry>> {
        let mut map: std::collections::HashMap<FileCategory, Vec<&FileEntry>> =
            std::collections::HashMap::new();
        for entry in &self.entries {
            if let Some(cat) = entry.category {
                map.entry(cat).or_default().push(entry);
            }
        }
        map
    }
}

// ─── Clean Plan ──────────────────────────────────────────────────────────────

/// An immutable plan describing what will be cleaned.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CleanPlan {
    /// Unique plan identifier.
    pub id: Uuid,
    /// Reference to the scan that produced this plan.
    pub scan_id: Uuid,
    /// When this plan was created.
    pub created_at: DateTime<Utc>,
    /// Actions to perform.
    pub actions: Vec<CleanAction>,
    /// Total space that will be freed (bytes).
    pub total_freeable: u64,
    /// Whether this is a dry-run plan.
    pub dry_run: bool,
}

/// A single cleanup action.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CleanAction {
    /// Unique action identifier.
    pub id: Uuid,
    /// Target path.
    pub path: PathBuf,
    /// Type of operation.
    pub operation: CleanOperation,
    /// File category.
    pub category: FileCategory,
    /// Risk level.
    pub risk: RiskLevel,
    /// Size that will be freed (bytes).
    pub size: u64,
    /// Human-readable reason for this action.
    pub reason: String,
    /// Current status.
    pub status: ActionStatus,
}

/// Type of cleanup operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CleanOperation {
    /// Permanently delete the file/directory.
    Delete,
    /// Move to OS trash/recycle bin.
    Trash,
    /// Move to quarantine archive.
    Compress,
    /// Delete a registry key (Windows only).
    DeleteRegistryKey,
}

/// Status of a cleanup action.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActionStatus {
    Pending,
    Approved,
    Skipped,
    InProgress,
    Completed,
    Failed,
}

// ─── App Info ────────────────────────────────────────────────────────────────

/// Information about an installed application.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppInfo {
    /// Application name.
    pub name: String,
    /// Publisher / developer.
    pub publisher: Option<String>,
    /// Installed version.
    pub version: Option<String>,
    /// Installation path.
    pub install_path: Option<PathBuf>,
    /// Install date.
    pub install_date: Option<DateTime<Utc>>,
    /// Estimated disk usage (bytes).
    pub size: Option<u64>,
    /// Last known usage time (heuristic).
    pub last_used: Option<DateTime<Utc>>,
    /// Whether the app appears to be unused.
    pub appears_unused: bool,
    /// Platform-specific identifier (registry key, bundle ID, package name).
    pub platform_id: Option<String>,
    /// Detected data directories.
    pub data_dirs: Vec<PathBuf>,
}

// ─── Disk Info ───────────────────────────────────────────────────────────────

/// Summary info for a mounted disk / volume.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiskInfo {
    /// Mount point / drive letter.
    pub mount_point: PathBuf,
    /// Volume name.
    pub name: String,
    /// Filesystem type (NTFS, ext4, APFS, etc.).
    pub fs_type: String,
    /// Total capacity in bytes.
    pub total_bytes: u64,
    /// Available space in bytes.
    pub available_bytes: u64,
    /// Whether this disk is removable.
    pub is_removable: bool,
}

impl DiskInfo {
    /// Used space in bytes.
    pub fn used_bytes(&self) -> u64 {
        self.total_bytes.saturating_sub(self.available_bytes)
    }

    /// Usage percentage (0.0–100.0).
    pub fn usage_percent(&self) -> f64 {
        if self.total_bytes == 0 {
            return 0.0;
        }
        (self.used_bytes() as f64 / self.total_bytes as f64) * 100.0
    }
}

// ─── Progress Events ─────────────────────────────────────────────────────────

/// Progress events emitted during scanning/cleaning operations.
#[derive(Debug, Clone, Serialize)]
pub enum ProgressEvent {
    /// Scan has started.
    ScanStarted { root: PathBuf },
    /// A directory is being scanned.
    ScanningDir { path: PathBuf },
    /// Periodic scan progress update.
    ScanProgress {
        files_scanned: u64,
        dirs_scanned: u64,
        bytes_scanned: u64,
        current_path: PathBuf,
    },
    /// A file could not be accessed.
    ScanError { path: PathBuf, error: String },
    /// Scan completed.
    ScanCompleted {
        result: ScanResult,
        duration_ms: u64,
    },
    /// Cleanup progress.
    CleanProgress {
        action_index: usize,
        total_actions: usize,
        current_path: PathBuf,
        bytes_freed: u64,
    },
    /// Cleanup completed.
    CleanCompleted {
        total_freed: u64,
        actions_completed: usize,
        actions_failed: usize,
    },
    /// Duplicate hashing progress.
    HashProgress {
        files_hashed: u64,
        total_to_hash: u64,
        current_path: PathBuf,
    },
}
