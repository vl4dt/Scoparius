//! TOML-based cleanup rule engine.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// A cleanup rule definition loaded from TOML.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CleanupRule {
    pub rule: RuleDefinition,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuleDefinition {
    pub name: String,
    pub description: String,
    pub category: String,
    pub risk: String,
    #[serde(default)]
    pub targets: Vec<RuleTarget>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuleTarget {
    pub platform: String,
    pub paths: Vec<String>,
    #[serde(default)]
    pub extensions: Vec<String>,
    #[serde(default)]
    pub patterns: Vec<String>,
}

/// Load all rules from a directory.
pub fn load_rules(dir: &std::path::Path) -> Vec<CleanupRule> {
    let mut rules = Vec::new();
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.filter_map(|e| e.ok()) {
            let path = entry.path();
            if path.extension().is_some_and(|ext| ext == "toml") {
                match std::fs::read_to_string(&path) {
                    Ok(contents) => match toml::from_str::<CleanupRule>(&contents) {
                        Ok(rule) => rules.push(rule),
                        Err(e) => tracing::warn!("Failed to parse rule {}: {}", path.display(), e),
                    },
                    Err(e) => tracing::warn!("Failed to read rule {}: {}", path.display(), e),
                }
            }
        }
    }
    rules
}

/// Expand platform-specific variables in rule paths.
pub fn expand_path(path: &str) -> PathBuf {
    let mut result = path.to_string();
    // Expand home directory
    if let Some(home) = dirs::home_dir() {
        result = result.replace("~", &home.to_string_lossy());
    }
    // Expand Windows env vars
    for var in &["LOCALAPPDATA", "APPDATA", "TEMP", "TMP", "WINDIR", "ProgramData", "USERPROFILE"] {
        if let Ok(val) = std::env::var(var) {
            result = result.replace(&format!("{{{var}}}"), &val);
        }
    }
    PathBuf::from(result)
}
