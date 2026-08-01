use serde_json::{json, Value};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// Incremental cache manager using file mtime and rule hash.
pub struct CacheManager {
    cache_dir: PathBuf,
    cache_data: HashMap<String, CacheEntry>,
}

#[derive(Debug, Clone)]
struct CacheEntry {
    /// File modification time (serialized as timestamp)
    mtime: u64,
    /// Hash of enabled rules (detects rule changes)
    rule_hash: String,
}

impl CacheManager {
    /// Create a cache manager with cache directory in node_modules/.custom-biome-lint-cache/
    pub fn new(cwd: &Path) -> Result<Self, String> {
        let cache_dir = cwd.join("node_modules/.custom-biome-lint-cache");
        Ok(Self {
            cache_dir,
            cache_data: HashMap::new(),
        })
    }

    /// Load cache from disk if it exists and is valid.
    pub fn load(&mut self) -> Result<(), String> {
        let cache_file = self.cache_dir.join("cache.json");
        if !cache_file.exists() {
            return Ok(()); // No cache yet, start fresh
        }

        match fs::read_to_string(&cache_file) {
            Ok(content) => {
                match serde_json::from_str::<Value>(&content) {
                    Ok(json) => {
                        if let Some(entries) = json.get("entries").and_then(|v| v.as_object()) {
                            for (path, entry) in entries {
                                if let (Some(mtime), Some(rule_hash)) =
                                    (entry.get("mtime").and_then(|v| v.as_u64()),
                                     entry.get("rule_hash").and_then(|v| v.as_str()))
                                {
                                    self.cache_data.insert(
                                        path.clone(),
                                        CacheEntry {
                                            mtime,
                                            rule_hash: rule_hash.to_string(),
                                        },
                                    );
                                }
                            }
                        }
                        Ok(())
                    }
                    Err(_) => {
                        // Corrupted cache, ignore and start fresh
                        Ok(())
                    }
                }
            }
            Err(_) => Ok(()), // Can't read, start fresh
        }
    }

    /// Check if a file is cached and valid (mtime unchanged, rules unchanged).
    pub fn is_valid(&self, path: &Path, rule_hash: &str) -> bool {
        let path_str = path.to_string_lossy().to_string();
        if let Some(entry) = self.cache_data.get(&path_str) {
            // Check mtime
            if let Ok(metadata) = fs::metadata(path) {
                if let Ok(mtime) = metadata.modified() {
                    let current_mtime = mtime
                        .duration_since(SystemTime::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs();
                    // Verify rule hash matches
                    return entry.mtime == current_mtime && entry.rule_hash == rule_hash;
                }
            }
        }
        false
    }

    /// Mark a file as cached with current mtime and rule hash.
    pub fn mark_valid(&mut self, path: &Path, rule_hash: &str) -> Result<(), String> {
        if let Ok(metadata) = fs::metadata(path) {
            if let Ok(mtime) = metadata.modified() {
                let mtime_secs = mtime
                    .duration_since(SystemTime::UNIX_EPOCH)
                    .map_err(|e| format!("Failed to get mtime: {e}"))?
                    .as_secs();
                let path_str = path.to_string_lossy().to_string();
                self.cache_data.insert(
                    path_str,
                    CacheEntry {
                        mtime: mtime_secs,
                        rule_hash: rule_hash.to_string(),
                    },
                );
                return Ok(());
            }
        }
        Err("Failed to get file metadata".to_string())
    }

    /// Save cache to disk.
    pub fn save(&self) -> Result<(), String> {
        // Create cache directory
        fs::create_dir_all(&self.cache_dir)
            .map_err(|e| format!("Failed to create cache directory: {e}"))?;

        // Build JSON
        let mut entries = serde_json::Map::new();
        for (path, entry) in &self.cache_data {
            entries.insert(
                path.clone(),
                json!({
                    "mtime": entry.mtime,
                    "rule_hash": entry.rule_hash,
                }),
            );
        }
        let cache_json = json!({
            "version": "1",
            "entries": entries,
        });

        // Write cache file
        let cache_file = self.cache_dir.join("cache.json");
        fs::write(&cache_file, cache_json.to_string())
            .map_err(|e| format!("Failed to write cache: {e}"))?;

        Ok(())
    }

    /// Get cache statistics.
    pub fn stats(&self) -> (usize, usize) {
        (self.cache_data.len(), 0) // (cached_files, cache_hits - computed per run)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn cache_manager_creates_empty_cache() {
        let tmpdir = TempDir::new().unwrap();
        let manager = CacheManager::new(tmpdir.path()).unwrap();
        assert_eq!(manager.stats().0, 0);
    }

    #[test]
    fn cache_marks_and_saves_files() {
        let tmpdir = TempDir::new().unwrap();
        let test_file = tmpdir.path().join("test.js");
        fs::write(&test_file, "code").unwrap();

        let mut manager = CacheManager::new(tmpdir.path()).unwrap();
        manager.mark_valid(&test_file, "rule-hash-v1").unwrap();
        manager.save().unwrap();

        // Verify cache file exists
        let cache_file = tmpdir.path().join("node_modules/.custom-biome-lint-cache/cache.json");
        assert!(cache_file.exists());
    }

    #[test]
    fn cache_detects_mtime_changes() {
        let tmpdir = TempDir::new().unwrap();
        let test_file = tmpdir.path().join("test.js");
        fs::write(&test_file, "code").unwrap();

        let mut manager = CacheManager::new(tmpdir.path()).unwrap();
        manager.mark_valid(&test_file, "rule-hash-v1").unwrap();
        assert!(manager.is_valid(&test_file, "rule-hash-v1"));

        // Modify file (sleep longer to ensure mtime changes on all filesystems)
        std::thread::sleep(std::time::Duration::from_secs(1));
        fs::write(&test_file, "modified code").unwrap();
        assert!(!manager.is_valid(&test_file, "rule-hash-v1"));
    }

    #[test]
    fn cache_detects_rule_hash_changes() {
        let tmpdir = TempDir::new().unwrap();
        let test_file = tmpdir.path().join("test.js");
        fs::write(&test_file, "code").unwrap();

        let mut manager = CacheManager::new(tmpdir.path()).unwrap();
        manager.mark_valid(&test_file, "rule-hash-v1").unwrap();
        assert!(manager.is_valid(&test_file, "rule-hash-v1"));

        // Different rule hash should invalidate
        assert!(!manager.is_valid(&test_file, "rule-hash-v2"));
    }

    #[test]
    fn cache_loads_from_disk() {
        let tmpdir = TempDir::new().unwrap();
        let test_file = tmpdir.path().join("test.js");
        fs::write(&test_file, "code").unwrap();

        // Save cache
        {
            let mut manager = CacheManager::new(tmpdir.path()).unwrap();
            manager.mark_valid(&test_file, "rule-hash-v1").unwrap();
            manager.save().unwrap();
        }

        // Load cache in new manager
        {
            let mut manager = CacheManager::new(tmpdir.path()).unwrap();
            manager.load().unwrap();
            assert_eq!(manager.stats().0, 1);
            assert!(manager.is_valid(&test_file, "rule-hash-v1"));
        }
    }

    #[test]
    fn corrupted_cache_is_recovered() {
        let tmpdir = TempDir::new().unwrap();
        let cache_dir = tmpdir.path().join("node_modules/.custom-biome-lint-cache");
        fs::create_dir_all(&cache_dir).unwrap();
        fs::write(cache_dir.join("cache.json"), "invalid json").unwrap();

        let mut manager = CacheManager::new(tmpdir.path()).unwrap();
        assert!(manager.load().is_ok()); // Should not error on corrupt cache
        assert_eq!(manager.stats().0, 0); // Cache is empty after loading corrupt file
    }
}
