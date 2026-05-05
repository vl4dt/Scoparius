//! Persistent storage and management for cleanup manifests and history.

use crate::error::{Result, ScopariusError};
use crate::models::CleanPlan;
use rusqlite::{params, Connection, OptionalExtension};
use std::path::{Path, PathBuf};
use uuid::Uuid;

/// Manages the persistence of cleanup plans (manifests) to a local SQLite database.
pub struct ManifestStore {
    conn: Connection,
}

impl ManifestStore {
    /// Open the manifest store at the specified database file path.
    pub fn open(db_path: &Path) -> Result<Self> {
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| ScopariusError::io(parent, e))?;
        }

        let conn = Connection::open(db_path)?;
        
        // Initialize schema
        conn.execute(
            "CREATE TABLE IF NOT EXISTS manifests (
                id TEXT PRIMARY KEY,
                scan_id TEXT NOT NULL,
                created_at TEXT NOT NULL,
                total_freeable INTEGER NOT NULL,
                dry_run INTEGER NOT NULL,
                data TEXT NOT NULL
            )",
            [],
        )?;

        Ok(Self { conn })
    }

    /// Save a cleanup plan to the store.
    pub fn save(&self, plan: &CleanPlan) -> Result<()> {
        let data = serde_json::to_string(plan)
            .map_err(|e| ScopariusError::Serialization(format!("Manifest serialization failed: {}", e)))?;

        self.conn.execute(
            "INSERT INTO manifests (id, scan_id, created_at, total_freeable, dry_run, data) 
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                plan.id.to_string(),
                plan.scan_id.to_string(),
                plan.created_at.to_rfc3339(),
                plan.total_freeable,
                if plan.dry_run { 1 } else { 0 },
                data,
            ],
        )?;

        Ok(())
    }

    /// List all saved manifests.
    pub fn list(&self) -> Result<Vec<CleanPlan>> {
        let mut stmt = self.conn.prepare(
            "SELECT data FROM manifests ORDER BY created_at DESC"
        )?;

        let plans = stmt.query_map([], |row| {
            let data: String = row.get(0)?;
            let plan: CleanPlan = serde_json::from_str(&data).map_err(|e| {
                rusqlite::Error::FromSqlConversionFailure(
                    0,
                    rusqlite::types::Type::Text,
                    Box::new(e),
                )
            })?;
            Ok(plan)
        })?;

        let mut result = Vec::new();
        for plan in plans {
            result.push(plan?);
        }
        Ok(result)
    }

    /// Get a specific manifest by ID.
    pub fn get(&self, id: Uuid) -> Result<Option<CleanPlan>> {
        let mut stmt = self.conn.prepare("SELECT data FROM manifests WHERE id = ?1")?;
        
        let data: Option<String> = stmt.query_row(params![id.to_string()], |row| row.get(0))
            .optional()?;

        match data {
            Some(json) => {
                let plan: CleanPlan = serde_json::from_str(&json)
                    .map_err(|e| ScopariusError::Serialization(format!("Manifest deserialization failed: {}", e)))?;
                Ok(Some(plan))
            }
            None => Ok(None),
        }
    }

    /// Delete a manifest from the store.
    pub fn delete(&self, id: Uuid) -> Result<()> {
        self.conn.execute("DELETE FROM manifests WHERE id = ?1", params![id.to_string()])?;
        Ok(())
    }
}

/// Helper to get the default manifest store path.
pub fn default_db_path() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("scoparius")
        .join("manifests.db")
}
