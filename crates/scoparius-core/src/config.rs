//! Configuration management for Scoparius.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Top-level Scoparius configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScopariusConfig {
    /// General settings.
    #[serde(default)]
    pub general: GeneralConfig,

    /// Scanner settings.
    #[serde(default)]
    pub scanner: ScannerConfig,

    /// Cleanup thresholds.
    #[serde(default)]
    pub thresholds: ThresholdConfig,

    /// LLM integration settings.
    #[serde(default)]
    pub llm: LlmConfig,

    /// Scheduling settings.
    #[serde(default)]
    pub schedule: ScheduleConfig,
}

impl Default for ScopariusConfig {
    fn default() -> Self {
        Self {
            general: GeneralConfig::default(),
            scanner: ScannerConfig::default(),
            thresholds: ThresholdConfig::default(),
            llm: LlmConfig::default(),
            schedule: ScheduleConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeneralConfig {
    /// Use trash/recycle bin instead of permanent delete.
    #[serde(default = "default_true")]
    pub use_trash: bool,

    /// Generate undo manifests for all operations.
    #[serde(default = "default_true")]
    pub generate_manifests: bool,

    /// Default output format: "table", "json", or "plain".
    #[serde(default = "default_output_format")]
    pub output_format: String,

    /// Path to custom rules directory.
    pub custom_rules_dir: Option<PathBuf>,

    /// Detect and warn about cloud-synced directories.
    #[serde(default = "default_true")]
    pub cloud_sync_detection: bool,

    /// Delay in milliseconds between I/O operations during cleanup.
    #[serde(default)]
    pub io_delay_ms: u64,

    /// Directory for compressed quarantined files.
    pub quarantine_dir: Option<PathBuf>,
}

impl Default for GeneralConfig {
    fn default() -> Self {
        Self {
            use_trash: true,
            generate_manifests: true,
            output_format: "table".into(),
            custom_rules_dir: None,
            cloud_sync_detection: true,
            io_delay_ms: 0,
            quarantine_dir: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScannerConfig {
    /// Maximum depth for directory traversal (0 = unlimited).
    #[serde(default)]
    pub max_depth: usize,

    /// Follow symbolic links.
    #[serde(default)]
    pub follow_symlinks: bool,

    /// Skip hidden files and directories.
    #[serde(default)]
    pub skip_hidden: bool,

    /// Minimum file size to include in scan (bytes).
    #[serde(default)]
    pub min_size: u64,

    /// Number of parallel threads.
    #[serde(default)]
    pub parallelism: usize,

    /// Paths/patterns to exclude.
    #[serde(default)]
    pub exclude_patterns: Vec<String>,

    /// Enable content hashing during scan (required for duplicate detection).
    #[serde(default)]
    pub enable_hashing: bool,
}

impl Default for ScannerConfig {
    fn default() -> Self {
        Self {
            max_depth: 0,
            follow_symlinks: false,
            skip_hidden: false,
            min_size: 0,
            parallelism: 0, // 0 = auto-detect
            exclude_patterns: Vec::new(),
            enable_hashing: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThresholdConfig {
    /// Days since last access for temp files.
    #[serde(default = "default_7")]
    pub temp_days: u64,

    /// Days since last access for cache.
    #[serde(default = "default_30")]
    pub cache_days: u64,

    /// Days since last access for logs.
    #[serde(default = "default_90")]
    pub log_days: u64,

    /// Days since last access for build artifacts.
    #[serde(default = "default_30")]
    pub build_artifact_days: u64,

    /// Days since last access for general stale files.
    #[serde(default = "default_180")]
    pub stale_days: u64,

    /// Minimum file size (bytes) to flag as "large".
    #[serde(default = "default_100mb")]
    pub large_file_bytes: u64,

    /// Days without app usage to flag as "unused".
    #[serde(default = "default_180")]
    pub unused_app_days: u64,
}

impl Default for ThresholdConfig {
    fn default() -> Self {
        Self {
            temp_days: 7,
            cache_days: 30,
            log_days: 90,
            build_artifact_days: 30,
            stale_days: 180,
            large_file_bytes: 100 * 1024 * 1024, // 100 MB
            unused_app_days: 180,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmConfig {
    /// Enable LLM integration.
    #[serde(default)]
    pub enabled: bool,

    /// Provider: "ollama", "openai", or "custom".
    #[serde(default = "default_ollama")]
    pub provider: String,

    /// Ollama/API base URL.
    #[serde(default = "default_ollama_url")]
    pub api_url: String,

    /// Model name.
    #[serde(default = "default_model")]
    pub model: String,

    /// API key (for OpenAI-compatible providers).
    pub api_key: Option<String>,
}

impl Default for LlmConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            provider: "ollama".into(),
            api_url: "http://localhost:11434".into(),
            model: "llama3.2".into(),
            api_key: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScheduleConfig {
    /// Enable automatic scheduled cleanup.
    #[serde(default)]
    pub enabled: bool,

    /// Cron expression or interval (e.g., "weekly", "daily", "0 3 * * 0").
    #[serde(default = "default_weekly")]
    pub interval: String,

    /// Only clean categories marked as "safe".
    #[serde(default = "default_true")]
    pub safe_only: bool,
}

impl Default for ScheduleConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            interval: "weekly".into(),
            safe_only: true,
        }
    }
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

impl ScopariusConfig {
    /// Load config from the default location.
    pub fn load() -> anyhow::Result<Self> {
        let config_path = Self::default_path();
        if config_path.exists() {
            let contents = std::fs::read_to_string(&config_path)?;
            let config: Self = toml::from_str(&contents)?;
            Ok(config)
        } else {
            Ok(Self::default())
        }
    }

    /// Load config from a specific file.
    pub fn load_from(path: &std::path::Path) -> anyhow::Result<Self> {
        let contents = std::fs::read_to_string(path)?;
        let config: Self = toml::from_str(&contents)?;
        Ok(config)
    }

    /// Save config to the default location.
    pub fn save(&self) -> anyhow::Result<()> {
        let path = Self::default_path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let contents = toml::to_string_pretty(self)?;
        std::fs::write(path, contents)?;
        Ok(())
    }

    /// Default config file path.
    pub fn default_path() -> PathBuf {
        dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("scoparius")
            .join("config.toml")
    }
}

fn default_true() -> bool { true }
fn default_output_format() -> String { "table".into() }
fn default_ollama() -> String { "ollama".into() }
fn default_ollama_url() -> String { "http://localhost:11434".into() }
fn default_model() -> String { "llama3.2".into() }
fn default_weekly() -> String { "weekly".into() }
fn default_7() -> u64 { 7 }
fn default_30() -> u64 { 30 }
fn default_90() -> u64 { 90 }
fn default_180() -> u64 { 180 }
fn default_100mb() -> u64 { 100 * 1024 * 1024 }
