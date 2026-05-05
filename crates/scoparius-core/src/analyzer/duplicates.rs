//! Duplicate file detection using size pre-filter and blake3 content hashing.

use crate::error::Result;
use crate::models::FileEntry;
use blake3::Hasher;
use std::collections::HashMap;
use std::io::Read;
use std::path::PathBuf;

/// A group of duplicate files sharing the same content hash.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DuplicateGroup {
    /// The blake3 content hash shared by all files.
    pub hash: String,
    /// Size of each file in the group (all identical).
    pub file_size: u64,
    /// Paths of all duplicate files.
    pub paths: Vec<PathBuf>,
    /// Total wasted space (count - 1) * size.
    pub wasted_bytes: u64,
}

/// Detect duplicate files from a list of file entries.
///
/// Uses a two-pass approach:
/// 1. Group files by size (cheap pre-filter)
/// 2. Hash only files with matching sizes (expensive, on-demand)
pub fn find_duplicates(
    entries: &[FileEntry],
    min_size: u64,
    progress_fn: Option<&dyn Fn(usize, usize)>,
) -> Result<Vec<DuplicateGroup>> {
    // Pass 1: Group by size
    let mut size_groups: HashMap<u64, Vec<&FileEntry>> = HashMap::new();
    for entry in entries {
        if entry.is_dir || entry.size < min_size {
            continue;
        }
        size_groups.entry(entry.size).or_default().push(entry);
    }

    // Filter to only size groups with 2+ files (potential duplicates)
    let candidates: Vec<_> = size_groups
        .into_iter()
        .filter(|(_, group)| group.len() >= 2)
        .collect();

    let total_to_hash: usize = candidates.iter().map(|(_, g)| g.len()).sum();
    let mut hashed = 0usize;

    // Pass 2: Hash candidates
    let mut hash_groups: HashMap<String, Vec<PathBuf>> = HashMap::new();
    let mut hash_sizes: HashMap<String, u64> = HashMap::new();

    for (_size, group) in &candidates {
        for entry in group {
            match hash_file(&entry.path) {
                Ok(hash) => {
                    hash_groups.entry(hash.clone()).or_default().push(entry.path.clone());
                    hash_sizes.entry(hash).or_insert(entry.size);
                }
                Err(e) => {
                    tracing::debug!("Failed to hash {}: {}", entry.path.display(), e);
                }
            }

            hashed += 1;
            if let Some(f) = progress_fn {
                f(hashed, total_to_hash);
            }
        }
    }

    // Build duplicate groups (only groups with 2+ matching hashes)
    let mut duplicates: Vec<DuplicateGroup> = hash_groups
        .into_iter()
        .filter(|(_, paths)| paths.len() >= 2)
        .map(|(hash, paths)| {
            let file_size = hash_sizes.get(&hash).copied().unwrap_or(0);
            let wasted = (paths.len() as u64 - 1) * file_size;
            DuplicateGroup {
                hash,
                file_size,
                paths,
                wasted_bytes: wasted,
            }
        })
        .collect();

    // Sort by wasted space descending
    duplicates.sort_by(|a, b| b.wasted_bytes.cmp(&a.wasted_bytes));

    tracing::info!(
        "Found {} duplicate groups, {} total wasted bytes",
        duplicates.len(),
        duplicates.iter().map(|d| d.wasted_bytes).sum::<u64>(),
    );

    Ok(duplicates)
}

/// Compute blake3 hash of a file, reading in 64KB chunks.
pub fn hash_file(path: &std::path::Path) -> std::io::Result<String> {
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Hasher::new();
    let mut buffer = [0u8; 65536];

    loop {
        let bytes_read = file.read(&mut buffer)?;
        if bytes_read == 0 {
            break;
        }
        hasher.update(&buffer[..bytes_read]);
    }

    Ok(hasher.finalize().to_hex().to_string())
}

/// Compute blake3 hash for a file entry, storing the result in the entry.
pub fn hash_entry(entry: &mut FileEntry) -> Result<()> {
    if entry.content_hash.is_some() || entry.is_dir {
        return Ok(());
    }
    match hash_file(&entry.path) {
        Ok(hash) => {
            entry.content_hash = Some(hash);
            Ok(())
        }
        Err(e) => {
            tracing::debug!("Failed to hash {}: {}", entry.path.display(), e);
            Ok(()) // Non-fatal — skip unhashable files
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::RiskLevel;
    use chrono::Utc;
    use tempfile::TempDir;

    fn make_entry(path: PathBuf, size: u64) -> FileEntry {
        FileEntry {
            path, size, is_dir: false,
            modified: Some(Utc::now()), accessed: Some(Utc::now()), created: None,
            content_hash: None, category: None, risk: RiskLevel::Safe,
        }
    }

    #[test]
    fn test_hash_file() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("test.txt");
        std::fs::write(&path, "hello world").unwrap();

        let hash1 = hash_file(&path).unwrap();
        let hash2 = hash_file(&path).unwrap();
        assert_eq!(hash1, hash2);
        assert!(!hash1.is_empty());
    }

    #[test]
    fn test_find_duplicates() {
        let tmp = TempDir::new().unwrap();

        // Create 3 files: 2 with same content, 1 different
        let path_a = tmp.path().join("a.txt");
        let path_b = tmp.path().join("b.txt");
        let path_c = tmp.path().join("c.txt");
        std::fs::write(&path_a, "duplicate content").unwrap();
        std::fs::write(&path_b, "duplicate content").unwrap();
        std::fs::write(&path_c, "unique content!!").unwrap();

        let size_dup = std::fs::metadata(&path_a).unwrap().len();
        let size_uniq = std::fs::metadata(&path_c).unwrap().len();

        let entries = vec![
            make_entry(path_a, size_dup),
            make_entry(path_b, size_dup),
            make_entry(path_c, size_uniq),
        ];

        let groups = find_duplicates(&entries, 0, None).unwrap();
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].paths.len(), 2);
        assert_eq!(groups[0].wasted_bytes, size_dup);
    }

    #[test]
    fn test_no_duplicates() {
        let tmp = TempDir::new().unwrap();
        let path_a = tmp.path().join("a.txt");
        let path_b = tmp.path().join("b.txt");
        std::fs::write(&path_a, "content one").unwrap();
        std::fs::write(&path_b, "content two").unwrap();

        let entries = vec![
            make_entry(path_a, 11),
            make_entry(path_b, 11),
        ];

        let groups = find_duplicates(&entries, 0, None).unwrap();
        assert!(groups.is_empty());
    }

    #[test]
    fn test_min_size_filter() {
        let tmp = TempDir::new().unwrap();
        let path_a = tmp.path().join("a.txt");
        let path_b = tmp.path().join("b.txt");
        std::fs::write(&path_a, "dup").unwrap();
        std::fs::write(&path_b, "dup").unwrap();

        let size = std::fs::metadata(&path_a).unwrap().len();
        let entries = vec![
            make_entry(path_a, size),
            make_entry(path_b, size),
        ];

        // min_size larger than file → no duplicates found
        let groups = find_duplicates(&entries, 1_000_000, None).unwrap();
        assert!(groups.is_empty());
    }
}
