//! Transactional cleanup engine.

use crate::config::ScopariusConfig;
use crate::error::{Result, ScopariusError};
use crate::models::*;
use chrono::Utc;
use std::path::PathBuf;
use std::time::Duration;
use tokio::sync::mpsc;
use uuid::Uuid;
use std::io::{Read, Write};
use zip::write::FileOptions;
use walkdir::WalkDir;

/// The cleanup engine that executes `CleanPlan`s.
pub struct Cleaner {
    config: ScopariusConfig,
}

impl Cleaner {
    pub fn new(config: ScopariusConfig) -> Self {
        Self { config }
    }

    /// Generate a cleanup plan from analyzed scan results.
    pub fn plan(&self, scan: &ScanResult, dry_run: bool) -> CleanPlan {
        let mut actions = Vec::new();
        for entry in &scan.entries {
            let Some(category) = entry.category else { continue };
            let operation = if self.config.general.use_trash {
                CleanOperation::Trash
            } else {
                CleanOperation::Delete
            };
            actions.push(CleanAction {
                id: Uuid::new_v4(),
                path: entry.path.clone(),
                operation,
                category,
                risk: entry.risk,
                size: entry.size,
                reason: format!("{}: {}", category.display_name(),
                    entry.path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default()),
                status: ActionStatus::Pending,
            });
        }
        let total_freeable = actions.iter().map(|a| a.size).sum();
        CleanPlan { id: Uuid::new_v4(), scan_id: scan.id, created_at: Utc::now(), actions, total_freeable, dry_run }
    }

    /// Execute a cleanup plan.
    pub async fn execute(
        &self, mut plan: CleanPlan, progress_tx: Option<mpsc::UnboundedSender<ProgressEvent>>,
    ) -> Result<CleanPlan> {
        if plan.dry_run {
            tracing::info!("Dry run — no files will be modified");
            return Ok(plan);
        }
        if self.config.general.generate_manifests { self.save_manifest(&plan)?; }

        let total = plan.actions.len();
        let (mut completed, mut failed, mut bytes_freed) = (0usize, 0usize, 0u64);

        for (i, action) in plan.actions.iter_mut().enumerate() {
            if action.status != ActionStatus::Pending && action.status != ActionStatus::Approved { continue; }
            if action.risk == RiskLevel::Critical { action.status = ActionStatus::Skipped; continue; }
            action.status = ActionStatus::InProgress;

            if let Some(tx) = &progress_tx {
                let _ = tx.send(ProgressEvent::CleanProgress {
                    action_index: i, total_actions: total, current_path: action.path.clone(), bytes_freed,
                });
            }

            // Rate limiting
            if self.config.general.io_delay_ms > 0 {
                tokio::time::sleep(Duration::from_millis(self.config.general.io_delay_ms)).await;
            }

            match self.execute_action(action).await {
                Ok(()) => { action.status = ActionStatus::Completed; bytes_freed += action.size; completed += 1; }
                Err(e) => { tracing::error!("Failed: {}: {}", action.path.display(), e); action.status = ActionStatus::Failed; failed += 1; }
            }
        }

        if let Some(tx) = &progress_tx {
            let _ = tx.send(ProgressEvent::CleanCompleted { total_freed: bytes_freed, actions_completed: completed, actions_failed: failed });
        }
        
        // Update manifest with final statuses
        if self.config.general.generate_manifests { self.save_manifest(&plan)?; }
        
        Ok(plan)
    }

    async fn execute_action(&self, action: &CleanAction) -> Result<()> {
        match action.operation {
            CleanOperation::Trash => trash::delete(&action.path).map_err(|e| ScopariusError::Cleaner(format!("Trash failed {}: {}", action.path.display(), e))),
            CleanOperation::Delete => {
                if action.path.is_dir() {
                    fs_err::remove_dir_all(&action.path).map_err(|e| ScopariusError::io(&action.path, e.into()))
                } else {
                    fs_err::remove_file(&action.path).map_err(|e| ScopariusError::io(&action.path, e.into()))
                }
            }
            CleanOperation::Compress => {
                let quarantine_dir = self.config.general.quarantine_dir.clone()
                    .unwrap_or_else(|| dirs::data_dir().unwrap_or_else(|| PathBuf::from(".")).join("scoparius").join("quarantine"));
                
                std::fs::create_dir_all(&quarantine_dir).map_err(|e| ScopariusError::io(&quarantine_dir, e))?;
                
                let zip_name = format!("{}_{}.zip", action.id, action.path.file_name().unwrap_or_default().to_string_lossy());
                let zip_path = quarantine_dir.join(zip_name);
                
                self.compress_path(&action.path, &zip_path)?;
                
                // After successful compression, delete the original
                if action.path.is_dir() {
                    fs_err::remove_dir_all(&action.path).map_err(|e| ScopariusError::io(&action.path, e.into()))?;
                } else {
                    fs_err::remove_file(&action.path).map_err(|e| ScopariusError::io(&action.path, e.into()))?;
                }
                Ok(())
            }
        }
    }

    fn compress_path(&self, src: &PathBuf, dest: &PathBuf) -> Result<()> {
        let file = std::fs::File::create(dest).map_err(|e| ScopariusError::io(dest, e))?;
        let mut zip = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated)
            .unix_permissions(0o755);

        if src.is_file() {
            #[allow(deprecated)]
            zip.start_file_from_path(src.file_name().unwrap_or_default(), options)
                .map_err(|e| ScopariusError::Cleaner(format!("Zip error: {}", e)))?;
            let mut f = std::fs::File::open(src).map_err(|e| ScopariusError::io(src, e))?;
            let mut buffer = Vec::new();
            f.read_to_end(&mut buffer).map_err(|e| ScopariusError::io(src, e))?;
            zip.write_all(&buffer).map_err(|e| ScopariusError::io(dest, e))?;
        } else if src.is_dir() {
            let walkdir = WalkDir::new(src);
            let it = walkdir.into_iter();
            for entry in it.filter_map(|e| e.ok()) {
                let path = entry.path();
                let name = path.strip_prefix(src).unwrap();
                let name_str = name.to_string_lossy().replace("\\", "/");
                
                if path.is_file() {
                    #[allow(deprecated)]
                    zip.start_file(name_str, options)
                        .map_err(|e| ScopariusError::Cleaner(format!("Zip error: {}", e)))?;
                    let mut f = std::fs::File::open(path).map_err(|e| ScopariusError::io(path, e))?;
                    let mut buffer = Vec::new();
                    f.read_to_end(&mut buffer).map_err(|e| ScopariusError::io(path, e))?;
                    zip.write_all(&buffer).map_err(|e| ScopariusError::io(dest, e))?;
                } else if !name.as_os_str().is_empty() {
                    #[allow(deprecated)]
                    zip.add_directory(name_str, options)
                        .map_err(|e| ScopariusError::Cleaner(format!("Zip error: {}", e)))?;
                }
            }
        }
        
        zip.finish().map_err(|e| ScopariusError::Cleaner(format!("Zip finish error: {}", e)))?;
        Ok(())
    }

    fn save_manifest(&self, plan: &CleanPlan) -> Result<()> {
        let dir = dirs::data_dir().unwrap_or_else(|| PathBuf::from(".")).join("scoparius").join("manifests");
        std::fs::create_dir_all(&dir).map_err(|e| ScopariusError::io(&dir, e))?;
        let path = dir.join(format!("{}.manifest.json", plan.id));
        let json = serde_json::to_string_pretty(plan).map_err(|e| ScopariusError::Serialization(e.to_string()))?;
        std::fs::write(&path, json).map_err(|e| ScopariusError::io(&path, e))?;
        tracing::info!("Manifest saved to {}", path.display());
        Ok(())
    }

    /// List all generated manifests
    pub fn list_manifests(&self) -> Result<Vec<CleanPlan>> {
        let dir = dirs::data_dir().unwrap_or_else(|| PathBuf::from(".")).join("scoparius").join("manifests");
        if !dir.exists() { return Ok(Vec::new()); }
        
        let mut plans = Vec::new();
        for entry in std::fs::read_dir(&dir).map_err(|e| ScopariusError::io(&dir, e))? {
            let entry = entry.map_err(|e| ScopariusError::io(&dir, e))?;
            let path = entry.path();
            if path.is_file() && path.extension().unwrap_or_default() == "json" {
                if let Ok(content) = std::fs::read_to_string(&path) {
                    if let Ok(plan) = serde_json::from_str::<CleanPlan>(&content) {
                        plans.push(plan);
                    }
                }
            }
        }
        plans.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        Ok(plans)
    }

    /// Read a specific manifest by ID
    pub fn read_manifest(&self, id: Uuid) -> Result<CleanPlan> {
        let dir = dirs::data_dir().unwrap_or_else(|| PathBuf::from(".")).join("scoparius").join("manifests");
        let path = dir.join(format!("{}.manifest.json", id));
        let content = std::fs::read_to_string(&path).map_err(|e| ScopariusError::io(&path, e))?;
        serde_json::from_str(&content).map_err(|e| ScopariusError::Serialization(e.to_string()))
    }

    /// Restore quarantined files from a specific manifest
    pub fn restore_manifest(&self, id: Uuid) -> Result<usize> {
        let plan = self.read_manifest(id)?;
        let quarantine_dir = self.config.general.quarantine_dir.clone()
            .unwrap_or_else(|| dirs::data_dir().unwrap_or_else(|| PathBuf::from(".")).join("scoparius").join("quarantine"));
        
        let mut restored = 0;
        
        for action in plan.actions {
            if action.status != ActionStatus::Completed { continue; }
            if action.operation == CleanOperation::Compress {
                let zip_name = format!("{}_{}.zip", action.id, action.path.file_name().unwrap_or_default().to_string_lossy());
                let zip_path = quarantine_dir.join(&zip_name);
                
                if zip_path.exists() {
                    let file = std::fs::File::open(&zip_path).map_err(|e| ScopariusError::io(&zip_path, e))?;
                    let mut archive = zip::ZipArchive::new(file).map_err(|e| ScopariusError::Cleaner(format!("Zip error: {}", e)))?;
                    
                    // For file actions, the zip contains exactly one file (the file itself).
                    // We should restore it exactly to action.path.
                    // For directories, the zip contains relative paths to action.path.
                    
                    if action.path.is_file() || archive.len() == 1 && action.path.extension().is_some() {
                        // This was a single file compression
                        if let Some(parent) = action.path.parent() {
                            std::fs::create_dir_all(parent).map_err(|e| ScopariusError::io(parent, e))?;
                        }
                        let mut file = archive.by_index(0).map_err(|e| ScopariusError::Cleaner(format!("Zip error: {}", e)))?;
                        let mut outfile = std::fs::File::create(&action.path).map_err(|e| ScopariusError::io(&action.path, e))?;
                        std::io::copy(&mut file, &mut outfile).map_err(|e| ScopariusError::io(&action.path, e))?;
                    } else {
                        // This was a directory compression
                        std::fs::create_dir_all(&action.path).map_err(|e| ScopariusError::io(&action.path, e))?;
                        for i in 0..archive.len() {
                            let mut file = archive.by_index(i).map_err(|e| ScopariusError::Cleaner(format!("Zip error: {}", e)))?;
                            let outpath = action.path.join(file.enclosed_name().unwrap());
                            
                            if (*file.name()).ends_with('/') {
                                std::fs::create_dir_all(&outpath).map_err(|e| ScopariusError::io(&outpath, e))?;
                            } else {
                                if let Some(p) = outpath.parent() {
                                    if !p.exists() {
                                        std::fs::create_dir_all(p).map_err(|e| ScopariusError::io(p, e))?;
                                    }
                                }
                                let mut outfile = std::fs::File::create(&outpath).map_err(|e| ScopariusError::io(&outpath, e))?;
                                std::io::copy(&mut file, &mut outfile).map_err(|e| ScopariusError::io(&outpath, e))?;
                            }
                        }
                    }
                    
                    // Cleanup quarantine zip after successful restore
                    let _ = std::fs::remove_file(&zip_path);
                    restored += 1;
                }
            }
        }
        
        Ok(restored)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_compression_and_restore() {
        let temp = tempdir().unwrap();
        let src_dir = temp.path().join("src");
        let quarantine_dir = temp.path().join("quarantine");
        fs::create_dir_all(&src_dir).unwrap();
        
        let test_file = src_dir.join("test.txt");
        fs::write(&test_file, "Hello, compression!").unwrap();

        let mut config = ScopariusConfig::default();
        config.general.quarantine_dir = Some(quarantine_dir.clone());
        config.general.generate_manifests = true;

        let cleaner = Cleaner::new(config);

        let plan = CleanPlan {
            id: Uuid::new_v4(),
            scan_id: Uuid::new_v4(),
            created_at: Utc::now(),
            actions: vec![CleanAction {
                id: Uuid::new_v4(),
                path: test_file.clone(),
                operation: CleanOperation::Compress,
                category: FileCategory::TempFile,
                risk: RiskLevel::Safe,
                size: 18,
                reason: "Test".into(),
                status: ActionStatus::Approved,
            }],
            total_freeable: 18,
            dry_run: false,
        };

        // Execution should compress and delete original
        let executed_plan = cleaner.execute(plan, None).await.unwrap();
        assert!(!test_file.exists(), "Original file should be deleted");

        // Verify quarantine zip exists
        let action = &executed_plan.actions[0];
        let zip_name = format!("{}_{}.zip", action.id, action.path.file_name().unwrap().to_string_lossy());
        assert!(quarantine_dir.join(&zip_name).exists(), "Zip file should exist in quarantine");

        // Now restore
        let restored_count = cleaner.restore_manifest(executed_plan.id).unwrap();
        assert_eq!(restored_count, 1);
        
        // Original should be back
        assert!(test_file.exists(), "File should be restored");
        assert_eq!(fs::read_to_string(&test_file).unwrap(), "Hello, compression!");
        
        // Zip should be gone
        assert!(!quarantine_dir.join(&zip_name).exists(), "Zip file should be cleaned up after restore");
    }
}
