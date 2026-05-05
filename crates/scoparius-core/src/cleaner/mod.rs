//! Core cleanup engine for executing CleanPlans.

pub mod manifest;

use crate::error::{Result, ScopariusError};
use crate::models::{ActionStatus, CleanAction, CleanOperation, ProgressEvent};
use crate::cleaner::manifest::{ManifestStore, default_db_path};
use std::fs;
use std::path::Path;
use tokio::sync::mpsc;

/// Executes cleanup operations defined in a CleanPlan.
pub struct Cleaner {
    dry_run: bool,
}

impl Cleaner {
    pub fn new(dry_run: bool) -> Self {
        Self { dry_run }
    }

    /// Create a cleanup plan from scan results.
    pub fn plan(&self, result: &crate::models::ScanResult, dry_run: bool) -> crate::models::CleanPlan {
        let mut actions = Vec::new();
        let mut total_freeable = 0;

        for entry in &result.entries {
            if let Some(category) = entry.category {
                let action = CleanAction {
                    id: uuid::Uuid::new_v4(),
                    path: entry.path.clone(),
                    operation: match category {
                        crate::models::FileCategory::Duplicate => CleanOperation::Delete,
                        _ => CleanOperation::Trash,
                    },
                    category,
                    risk: entry.risk,
                    size: entry.size,
                    reason: format!("Categorized as {:?}", category),
                    status: ActionStatus::Pending,
                };
                total_freeable += entry.size;
                actions.push(action);
            }
        }

        crate::models::CleanPlan {
            id: uuid::Uuid::new_v4(),
            scan_id: result.id,
            created_at: chrono::Utc::now(),
            actions,
            total_freeable,
            dry_run,
        }
    }

    /// Execute a full cleanup plan.
    pub async fn execute(
        &self,
        mut plan: crate::models::CleanPlan,
        progress_tx: Option<mpsc::UnboundedSender<ProgressEvent>>,
    ) -> Result<()> {
        execute_cleanup(&mut plan.actions, plan.dry_run, progress_tx).await?;
        
        // Save the completed plan to the manifest store
        self.save_manifest(&plan)?;
        
        Ok(())
    }

    /// Execute a single cleanup action.
    pub fn execute_action(&self, action: &mut CleanAction) -> Result<()> {
        if self.dry_run {
            action.status = ActionStatus::Completed;
            return Ok(());
        }

        action.status = ActionStatus::InProgress;
        
        let res = match action.operation {
            CleanOperation::Delete => self.delete_file(&action.path),
            CleanOperation::Trash => self.trash_file(&action.path),
            CleanOperation::Compress => Err(ScopariusError::Cleaner("Compression not implemented".into())),
            CleanOperation::DeleteRegistryKey => Err(ScopariusError::Cleaner("Registry cleanup not implemented".into())),
        };

        match res {
            Ok(_) => {
                action.status = ActionStatus::Completed;
                Ok(())
            }
            Err(e) => {
                action.status = ActionStatus::Failed;
                Err(e)
            }
        }
    }

    fn delete_file(&self, path: &Path) -> Result<()> {
        if !path.exists() {
            return Ok(());
        }

        if path.is_dir() {
            fs::remove_dir_all(path).map_err(|e| ScopariusError::io(path, e))
        } else {
            fs::remove_file(path).map_err(|e| ScopariusError::io(path, e))
        }
    }

    fn trash_file(&self, path: &Path) -> Result<()> {
        #[cfg(target_os = "windows")]
        {
            trash::delete(path).map_err(|e| ScopariusError::Cleaner(format!("Recycle Bin error: {}", e)))
        }
        #[cfg(not(target_os = "windows"))]
        {
            // Fallback to delete if trash is not available/implemented for platform
            self.delete_file(path)
        }
    }
}

/// Executes a batch of actions and reports progress.
pub async fn execute_cleanup(
    actions: &mut [CleanAction],
    dry_run: bool,
    progress_tx: Option<mpsc::UnboundedSender<ProgressEvent>>,
) -> Result<()> {
    let cleaner = Cleaner::new(dry_run);
    let total_actions = actions.len();
    let mut bytes_freed = 0u64;
    let mut actions_completed = 0;
    let mut actions_failed = 0;

    for (i, action) in actions.iter_mut().enumerate() {
        let path = action.path.clone();
        let size = action.size;

        match cleaner.execute_action(action) {
            Ok(_) => {
                bytes_freed += size;
                actions_completed += 1;
            }
            Err(_) => {
                actions_failed += 1;
            }
        }

        if let Some(tx) = &progress_tx {
            let _ = tx.send(ProgressEvent::CleanProgress {
                action_index: i + 1,
                total_actions,
                current_path: path,
                bytes_freed,
            });
        }
    }

    if let Some(tx) = &progress_tx {
        let _ = tx.send(ProgressEvent::CleanCompleted {
            total_freed: bytes_freed,
            actions_completed,
            actions_failed,
        });
    }

    Ok(())
}


// ─── Manifests & Restoration ────────────────────────────────────────────────

impl Cleaner {
    /// Save a cleanup plan to the manifest store.
    pub fn save_manifest(&self, plan: &crate::models::CleanPlan) -> Result<()> {
        let store = ManifestStore::open(&default_db_path())?;
        store.save(plan)
    }

    /// List all saved manifests.
    pub fn list_manifests(&self) -> Result<Vec<crate::models::CleanPlan>> {
        let store = ManifestStore::open(&default_db_path())?;
        store.list()
    }

    /// Read a specific manifest by ID.
    pub fn read_manifest(&self, id: uuid::Uuid) -> Result<crate::models::CleanPlan> {
        let store = ManifestStore::open(&default_db_path())?;
        store.get(id)?
            .ok_or_else(|| ScopariusError::Cleaner(format!("Manifest {} not found", id)))
    }

    /// Restore files from a manifest.
    /// This is a complex operation that depends on the action types (Trash vs Delete).
    pub fn restore_manifest(&self, id: uuid::Uuid) -> Result<usize> {
        let plan = self.read_manifest(id)?;
        let mut restored = 0;

        for action in &plan.actions {
            if action.status != ActionStatus::Completed {
                continue;
            }

            match action.operation {
                CleanOperation::Trash => {
                    // Note: 'trash' crate doesn't easily support programmatic restoration from bin
                    // across all platforms. We log this as a limitation for now.
                    tracing::warn!("Programmatic restoration from Trash/Recycle Bin is OS-dependent and limited. Please use system tools to restore: {}", action.path.display());
                }
                CleanOperation::Compress => {
                    // If we implemented quarantine via ZIP, we would extract here
                    if let Ok(count) = self.restore_from_quarantine(action) {
                        restored += count;
                    }
                }
                CleanOperation::Delete | CleanOperation::DeleteRegistryKey => {
                    // Cannot restore permanently deleted items
                    tracing::error!("Cannot restore permanently deleted item: {}", action.path.display());
                }
            }
        }

        Ok(restored)
    }

    fn restore_from_quarantine(&self, _action: &CleanAction) -> Result<usize> {
        // Implementation for unzipping from a quarantine folder
        Ok(0)
    }
}
