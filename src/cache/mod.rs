use serde_json::{json, Value};
use std::collections::hash_map::DefaultHasher;
use std::collections::HashMap;
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

/// Incremental cache manager keyed on file content hash, not mtime.
///
/// mtime-based invalidation looks free (no need to read the file to check
/// it), but a fresh checkout — the common case in CI — gives every file a
/// new mtime regardless of whether its content changed, which would defeat
/// the cache on exactly the runs where it matters most. Content hashing
/// costs a full read of every candidate file every run, but that read was
/// already happening for any file whose cache the tool needed to actually
/// check; the win this cache exists for is skipping the *parse and rule
/// execution* that follows, which is where ~70% of run time goes (see
/// docs/ARCHITECTURE.md), not skipping the read itself.
pub struct CacheManager {
    cache_dir: PathBuf,
    cache_data: HashMap<String, CacheEntry>,
}

#[derive(Debug, Clone)]
struct CacheEntry {
    /// Hash of the file's content at the time it was last found clean.
    content_hash: String,
    /// Hash of the enabled rule set and tool version (see
    /// [`compute_cache_key`] in `cli::mod`) — invalidates every cached file
    /// at once when either changes.
    cache_key: String,
}

/// Hashes file content for cache-key purposes only. Not cryptographic: it
/// only needs a low collision rate for this tool's own cache, computed over
/// content already held in memory (the file was read to be linted either
/// way), not a separate pass over the filesystem.
pub fn hash_content(source: &str) -> String {
    let mut hasher = DefaultHasher::new();
    source.hash(&mut hasher);
    format!("{:x}", hasher.finish())
}

impl CacheManager {
    /// Create a cache manager with cache directory in .custom-biome-lint-cache/,
    /// created on first run. Not nested under node_modules — this tool has no
    /// npm dependencies of its own and shouldn't imply that it does.
    pub fn new(cwd: &Path) -> Result<Self, String> {
        let cache_dir = cwd.join(".custom-biome-lint-cache");
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
                                if let (Some(content_hash), Some(cache_key)) = (
                                    entry.get("content_hash").and_then(|v| v.as_str()),
                                    entry.get("cache_key").and_then(|v| v.as_str()),
                                ) {
                                    self.cache_data.insert(
                                        path.clone(),
                                        CacheEntry {
                                            content_hash: content_hash.to_string(),
                                            cache_key: cache_key.to_string(),
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

    /// Whether `path`'s already-hashed content and the current cache key
    /// (enabled rules + tool version) both match what was last cached.
    pub fn is_valid(&self, path: &Path, content_hash: &str, cache_key: &str) -> bool {
        let path_str = path.to_string_lossy().to_string();
        self.cache_data
            .get(&path_str)
            .is_some_and(|entry| entry.content_hash == content_hash && entry.cache_key == cache_key)
    }

    /// Mark a file as cached with its current content hash and cache key.
    pub fn mark_valid(&mut self, path: &Path, content_hash: &str, cache_key: &str) {
        let path_str = path.to_string_lossy().to_string();
        self.cache_data.insert(
            path_str,
            CacheEntry {
                content_hash: content_hash.to_string(),
                cache_key: cache_key.to_string(),
            },
        );
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
                    "content_hash": entry.content_hash,
                    "cache_key": entry.cache_key,
                }),
            );
        }
        let cache_json = json!({
            "version": "2",
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

    /// Path to the cache directory (where cache.json is written on save()).
    pub fn cache_dir(&self) -> &Path {
        &self.cache_dir
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
        manager.mark_valid(&test_file, &hash_content("code"), "rule-hash-v1");
        manager.save().unwrap();

        // Verify cache file exists
        let cache_file = tmpdir.path().join(".custom-biome-lint-cache/cache.json");
        assert!(cache_file.exists());
    }

    #[test]
    fn cache_detects_content_changes_even_with_an_unchanged_mtime() {
        let tmpdir = TempDir::new().unwrap();
        let test_file = tmpdir.path().join("test.js");
        fs::write(&test_file, "code").unwrap();

        let mut manager = CacheManager::new(tmpdir.path()).unwrap();
        manager.mark_valid(&test_file, &hash_content("code"), "rule-hash-v1");
        assert!(manager.is_valid(&test_file, &hash_content("code"), "rule-hash-v1"));

        // Different content hash invalidates regardless of what happened to
        // the file's mtime -- there is no mtime check left to fool.
        assert!(!manager.is_valid(&test_file, &hash_content("modified code"), "rule-hash-v1"));
    }

    #[test]
    fn identical_content_stays_valid_even_if_rewritten() {
        // Simulates a fresh checkout: the file is rewritten (a real mtime
        // bump would occur here on a real filesystem) but with byte-for-byte
        // identical content, so the content hash -- and therefore validity
        // -- is unchanged.
        let tmpdir = TempDir::new().unwrap();
        let test_file = tmpdir.path().join("test.js");
        fs::write(&test_file, "code").unwrap();

        let mut manager = CacheManager::new(tmpdir.path()).unwrap();
        let hash = hash_content("code");
        manager.mark_valid(&test_file, &hash, "rule-hash-v1");

        fs::write(&test_file, "code").unwrap();
        assert!(manager.is_valid(&test_file, &hash_content("code"), "rule-hash-v1"));
    }

    #[test]
    fn cache_detects_rule_hash_changes() {
        let tmpdir = TempDir::new().unwrap();
        let test_file = tmpdir.path().join("test.js");
        fs::write(&test_file, "code").unwrap();

        let mut manager = CacheManager::new(tmpdir.path()).unwrap();
        let hash = hash_content("code");
        manager.mark_valid(&test_file, &hash, "rule-hash-v1");
        assert!(manager.is_valid(&test_file, &hash, "rule-hash-v1"));

        // Different rule hash should invalidate
        assert!(!manager.is_valid(&test_file, &hash, "rule-hash-v2"));
    }

    #[test]
    fn cache_loads_from_disk() {
        let tmpdir = TempDir::new().unwrap();
        let test_file = tmpdir.path().join("test.js");
        fs::write(&test_file, "code").unwrap();
        let hash = hash_content("code");

        // Save cache
        {
            let mut manager = CacheManager::new(tmpdir.path()).unwrap();
            manager.mark_valid(&test_file, &hash, "rule-hash-v1");
            manager.save().unwrap();
        }

        // Load cache in new manager
        {
            let mut manager = CacheManager::new(tmpdir.path()).unwrap();
            manager.load().unwrap();
            assert_eq!(manager.stats().0, 1);
            assert!(manager.is_valid(&test_file, &hash, "rule-hash-v1"));
        }
    }

    #[test]
    fn corrupted_cache_is_recovered() {
        let tmpdir = TempDir::new().unwrap();
        let cache_dir = tmpdir.path().join(".custom-biome-lint-cache");
        fs::create_dir_all(&cache_dir).unwrap();
        fs::write(cache_dir.join("cache.json"), "invalid json").unwrap();

        let mut manager = CacheManager::new(tmpdir.path()).unwrap();
        assert!(manager.load().is_ok()); // Should not error on corrupt cache
        assert_eq!(manager.stats().0, 0); // Cache is empty after loading corrupt file
    }

    #[test]
    fn hash_content_is_deterministic_and_sensitive_to_every_byte() {
        assert_eq!(hash_content("const a = 1;"), hash_content("const a = 1;"));
        assert_ne!(hash_content("const a = 1;"), hash_content("const a = 2;"));
    }

    #[test]
    fn old_mtime_format_cache_is_silently_ignored_not_misread() {
        // A cache.json written by the pre-content-hash version of this tool
        // has "mtime"/"rule_hash" fields, not "content_hash"/"cache_key".
        // load() must skip entries missing the new fields rather than
        // somehow treating the old shape as valid.
        let tmpdir = TempDir::new().unwrap();
        let cache_dir = tmpdir.path().join(".custom-biome-lint-cache");
        fs::create_dir_all(&cache_dir).unwrap();
        fs::write(
            cache_dir.join("cache.json"),
            r#"{"version":"1","entries":{"/some/file.js":{"mtime":123,"rule_hash":"abc"}}}"#,
        )
        .unwrap();

        let mut manager = CacheManager::new(tmpdir.path()).unwrap();
        manager.load().unwrap();
        assert_eq!(
            manager.stats().0,
            0,
            "old-format entry must not be loaded as valid"
        );
    }
}
