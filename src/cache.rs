use crate::desktop_parser::DesktopFile;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

/// Trait for desktop file caching strategies
pub trait DesktopCache {
    /// Load the cache from storage
    fn load(&mut self) -> Result<()>;

    /// Save the cache to storage
    fn save(&self) -> Result<()>;

    /// Get a desktop file from the cache
    #[allow(dead_code)]
    fn get(&self, path: &Path) -> Option<&DesktopFile>;

    /// Insert a desktop file into the cache
    fn insert(&mut self, path: PathBuf, desktop_file: DesktopFile);

    /// Remove a desktop file from the cache
    #[allow(dead_code)]
    fn remove(&mut self, path: &Path) -> Option<DesktopFile>;

    /// Clear all entries from the cache
    fn clear(&mut self);

    /// Check if the cache is empty
    fn is_empty(&self) -> bool;

    /// Get the number of entries in the cache
    fn len(&self) -> usize;

    /// Get all entries in the cache
    fn iter(&self) -> Box<dyn Iterator<Item = (&PathBuf, &DesktopFile)> + '_>;

    /// Check if cache needs invalidation
    fn needs_invalidation(&self) -> bool;

    /// Invalidate expired entries
    fn invalidate_expired(&mut self);
}

/// Cache entry with metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
struct CacheEntry {
    desktop_file: DesktopFile,
    last_modified: SystemTime,
    cached_at: SystemTime,
}

impl CacheEntry {
    fn new(desktop_file: DesktopFile, last_modified: SystemTime) -> Self {
        Self {
            desktop_file,
            last_modified,
            cached_at: SystemTime::now(),
        }
    }

    fn is_expired(&self, file_path: &Path, max_age: Duration) -> bool {
        match fs::metadata(file_path) {
            Ok(metadata) => {
                if let Ok(modified) = metadata.modified() {
                    if modified > self.last_modified {
                        return true;
                    }
                }
            }
            Err(_) => return true,
        }

        // Check if cache entry is too old
        if let Ok(elapsed) = self.cached_at.elapsed() {
            elapsed > max_age
        } else {
            true // If we can't determine age, consider it expired
        }
    }
}

/// File system-based cache implementation
#[derive(Debug)]
pub struct FileSystemCache {
    cache_path: PathBuf,
    entries: HashMap<PathBuf, CacheEntry>,
    max_age: Duration,
}

impl FileSystemCache {
    pub fn new(cache_path: PathBuf) -> Self {
        Self {
            cache_path,
            entries: HashMap::new(),
            max_age: Duration::from_secs(24 * 60 * 60), // 24 hours
        }
    }

    #[allow(dead_code)]
    pub fn with_max_age(cache_path: PathBuf, max_age: Duration) -> Self {
        Self {
            cache_path,
            entries: HashMap::new(),
            max_age,
        }
    }
}

impl DesktopCache for FileSystemCache {
    fn load(&mut self) -> Result<()> {
        if !self.cache_path.exists() {
            return Ok(());
        }

        let contents = fs::read_to_string(&self.cache_path).context("Failed to read cache file")?;

        self.entries = serde_json::from_str(&contents).context("Failed to parse cache file")?;

        // Remove expired entries after loading
        self.invalidate_expired();

        Ok(())
    }

    fn save(&self) -> Result<()> {
        if let Some(parent) = self.cache_path.parent() {
            fs::create_dir_all(parent).context("Failed to create cache directory")?;
        }

        let json = serde_json::to_string(&self.entries).context("Failed to serialize cache")?;

        fs::write(&self.cache_path, json).context("Failed to write cache file")?;

        Ok(())
    }

    fn get(&self, path: &Path) -> Option<&DesktopFile> {
        self.entries.get(path).map(|entry| &entry.desktop_file)
    }

    fn insert(&mut self, path: PathBuf, desktop_file: DesktopFile) {
        let last_modified = fs::metadata(&path)
            .and_then(|m| m.modified())
            .unwrap_or_else(|_| SystemTime::now());

        let entry = CacheEntry::new(desktop_file, last_modified);
        self.entries.insert(path, entry);
    }

    fn remove(&mut self, path: &Path) -> Option<DesktopFile> {
        self.entries.remove(path).map(|entry| entry.desktop_file)
    }

    fn clear(&mut self) {
        self.entries.clear();
    }

    fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    fn len(&self) -> usize {
        self.entries.len()
    }

    fn iter(&self) -> Box<dyn Iterator<Item = (&PathBuf, &DesktopFile)> + '_> {
        Box::new(
            self.entries
                .iter()
                .map(|(path, entry)| (path, &entry.desktop_file)),
        )
    }

    fn needs_invalidation(&self) -> bool {
        self.entries
            .iter()
            .any(|(path, entry)| entry.is_expired(path, self.max_age))
    }

    fn invalidate_expired(&mut self) {
        let max_age = self.max_age;
        self.entries
            .retain(|path, entry| !entry.is_expired(path, max_age));
    }
}

/// In-memory cache implementation
#[derive(Debug)]
pub struct MemoryCache {
    entries: HashMap<PathBuf, DesktopFile>,
}

impl MemoryCache {
    pub fn new() -> Self {
        Self {
            entries: HashMap::new(),
        }
    }
}

impl Default for MemoryCache {
    fn default() -> Self {
        Self::new()
    }
}

impl DesktopCache for MemoryCache {
    fn load(&mut self) -> Result<()> {
        // Memory cache doesn't persist, so loading is a no-op
        Ok(())
    }

    fn save(&self) -> Result<()> {
        // Memory cache doesn't persist, so saving is a no-op
        Ok(())
    }

    fn get(&self, path: &Path) -> Option<&DesktopFile> {
        self.entries.get(path)
    }

    fn insert(&mut self, path: PathBuf, desktop_file: DesktopFile) {
        self.entries.insert(path, desktop_file);
    }

    fn remove(&mut self, path: &Path) -> Option<DesktopFile> {
        self.entries.remove(path)
    }

    fn clear(&mut self) {
        self.entries.clear();
    }

    fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    fn len(&self) -> usize {
        self.entries.len()
    }

    fn iter(&self) -> Box<dyn Iterator<Item = (&PathBuf, &DesktopFile)> + '_> {
        Box::new(self.entries.iter())
    }

    fn needs_invalidation(&self) -> bool {
        // Memory cache doesn't track file modification times
        false
    }

    fn invalidate_expired(&mut self) {
        // Memory cache doesn't have expiration
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::desktop_parser::DesktopEntry;
    use std::collections::HashMap;
    use std::thread;
    use tempfile::TempDir;

    fn create_test_desktop_file() -> DesktopFile {
        let entry = DesktopEntry {
            name: "Test App".to_string(),
            exec: "testapp %F".to_string(),
            comment: Some("Test application".to_string()),
            icon: Some("test-icon".to_string()),
            mime_types: vec!["text/plain".to_string()],
            ..DesktopEntry::default()
        };

        DesktopFile {
            main_entry: Some(entry),
            actions: HashMap::new(),
        }
    }

    #[test]
    fn test_memory_cache_basic_operations() {
        let mut cache = MemoryCache::new();
        let desktop_file = create_test_desktop_file();
        let path = PathBuf::from("/test/app.desktop");

        // Test empty cache
        assert!(cache.is_empty());
        assert_eq!(cache.len(), 0);
        assert!(cache.get(&path).is_none());

        // Test insert and get
        cache.insert(path.clone(), desktop_file.clone());
        assert!(!cache.is_empty());
        assert_eq!(cache.len(), 1);
        assert!(cache.get(&path).is_some());

        // Test remove
        let removed = cache.remove(&path);
        assert!(removed.is_some());
        assert!(cache.is_empty());
        assert!(cache.get(&path).is_none());
    }

    #[test]
    fn test_memory_cache_clear() {
        let mut cache = MemoryCache::new();
        let desktop_file = create_test_desktop_file();

        cache.insert(PathBuf::from("/test1.desktop"), desktop_file.clone());
        cache.insert(PathBuf::from("/test2.desktop"), desktop_file);

        assert_eq!(cache.len(), 2);

        cache.clear();
        assert!(cache.is_empty());
        assert_eq!(cache.len(), 0);
    }

    #[test]
    fn test_memory_cache_iter() {
        let mut cache = MemoryCache::new();
        let desktop_file = create_test_desktop_file();

        let path1 = PathBuf::from("/test1.desktop");
        let path2 = PathBuf::from("/test2.desktop");

        cache.insert(path1.clone(), desktop_file.clone());
        cache.insert(path2.clone(), desktop_file);

        let entries: Vec<_> = cache.iter().collect();
        assert_eq!(entries.len(), 2);

        let paths: Vec<PathBuf> = entries.iter().map(|(path, _)| (*path).clone()).collect();
        assert!(paths.contains(&path1));
        assert!(paths.contains(&path2));
    }

    #[test]
    fn test_memory_cache_load_save() {
        let mut cache = MemoryCache::new();

        // Load and save should be no-ops for memory cache
        assert!(cache.load().is_ok());
        assert!(cache.save().is_ok());
    }

    #[test]
    fn test_memory_cache_invalidation() {
        let cache = MemoryCache::new();

        // Memory cache doesn't support invalidation
        assert!(!cache.needs_invalidation());

        let mut cache = cache;
        cache.invalidate_expired(); // Should not panic
    }

    #[test]
    fn test_filesystem_cache_basic_operations() {
        let temp_dir = TempDir::new().unwrap();
        let cache_path = temp_dir.path().join("cache.json");
        let mut cache = FileSystemCache::new(cache_path);

        let desktop_file = create_test_desktop_file();
        let path = PathBuf::from("/test/app.desktop");

        // Test empty cache
        assert!(cache.is_empty());
        assert_eq!(cache.len(), 0);
        assert!(cache.get(&path).is_none());

        // Test insert and get
        cache.insert(path.clone(), desktop_file.clone());
        assert!(!cache.is_empty());
        assert_eq!(cache.len(), 1);
        assert!(cache.get(&path).is_some());

        // Test remove
        let removed = cache.remove(&path);
        assert!(removed.is_some());
        assert!(cache.is_empty());
        assert!(cache.get(&path).is_none());
    }

    #[test]
    fn test_filesystem_cache_persistence() {
        let temp_dir = TempDir::new().unwrap();
        let cache_path = temp_dir.path().join("cache.json");
        let desktop_file = create_test_desktop_file();
        let path = temp_dir.path().join("app.desktop");
        fs::write(&path, "[Desktop Entry]").unwrap();

        // Create cache and add entry
        {
            let mut cache = FileSystemCache::new(cache_path.clone());
            cache.insert(path.clone(), desktop_file.clone());
            assert!(cache.save().is_ok());
        }

        // Load cache in new instance
        {
            let mut cache = FileSystemCache::new(cache_path);
            assert!(cache.load().is_ok());
            assert_eq!(cache.len(), 1);
            assert!(cache.get(&path).is_some());
        }
    }

    #[test]
    fn test_filesystem_cache_load_nonexistent() {
        let temp_dir = TempDir::new().unwrap();
        let cache_path = temp_dir.path().join("nonexistent.json");
        let mut cache = FileSystemCache::new(cache_path);

        // Loading non-existent cache should succeed
        assert!(cache.load().is_ok());
        assert!(cache.is_empty());
    }

    #[test]
    fn test_filesystem_cache_load_invalid_json() {
        let temp_dir = TempDir::new().unwrap();
        let cache_path = temp_dir.path().join("invalid.json");

        // Write invalid JSON
        fs::write(&cache_path, "invalid json").unwrap();

        let mut cache = FileSystemCache::new(cache_path);

        // Loading invalid JSON should fail
        assert!(cache.load().is_err());
    }

    #[test]
    fn test_filesystem_cache_with_max_age() {
        let temp_dir = TempDir::new().unwrap();
        let cache_path = temp_dir.path().join("cache.json");
        let max_age = Duration::from_millis(100);
        let mut cache = FileSystemCache::with_max_age(cache_path, max_age);

        let desktop_file = create_test_desktop_file();
        let path = PathBuf::from("/test/app.desktop");

        cache.insert(path.clone(), desktop_file);
        assert_eq!(cache.len(), 1);

        // Wait for expiration
        thread::sleep(Duration::from_millis(150));

        // Cache should need invalidation
        assert!(cache.needs_invalidation());

        // Invalidate expired entries
        cache.invalidate_expired();
        assert!(cache.is_empty());
    }

    #[test]
    fn test_filesystem_cache_iter() {
        let temp_dir = TempDir::new().unwrap();
        let cache_path = temp_dir.path().join("cache.json");
        let mut cache = FileSystemCache::new(cache_path);

        let desktop_file = create_test_desktop_file();
        let path1 = PathBuf::from("/test1.desktop");
        let path2 = PathBuf::from("/test2.desktop");

        cache.insert(path1.clone(), desktop_file.clone());
        cache.insert(path2.clone(), desktop_file);

        let entries: Vec<_> = cache.iter().collect();
        assert_eq!(entries.len(), 2);

        let paths: Vec<PathBuf> = entries.iter().map(|(path, _)| (*path).clone()).collect();
        assert!(paths.contains(&path1));
        assert!(paths.contains(&path2));
    }

    #[test]
    fn test_filesystem_cache_clear() {
        let temp_dir = TempDir::new().unwrap();
        let cache_path = temp_dir.path().join("cache.json");
        let mut cache = FileSystemCache::new(cache_path);

        let desktop_file = create_test_desktop_file();
        cache.insert(PathBuf::from("/test1.desktop"), desktop_file.clone());
        cache.insert(PathBuf::from("/test2.desktop"), desktop_file);

        assert_eq!(cache.len(), 2);

        cache.clear();
        assert!(cache.is_empty());
        assert_eq!(cache.len(), 0);
    }

    #[test]
    fn test_cache_entry_expiration() {
        let temp_dir = TempDir::new().unwrap();
        let test_file = temp_dir.path().join("test.desktop");

        // Create a test file
        fs::write(&test_file, "test content").unwrap();
        let metadata = fs::metadata(&test_file).unwrap();
        let last_modified = metadata.modified().unwrap();

        let desktop_file = create_test_desktop_file();
        let entry = CacheEntry::new(desktop_file, last_modified);

        // Entry should not be expired immediately
        assert!(!entry.is_expired(&test_file, Duration::from_secs(60)));

        // Entry should be expired with very short max age
        thread::sleep(Duration::from_millis(10));
        assert!(entry.is_expired(&test_file, Duration::from_millis(5)));
    }

    #[test]
    fn test_cache_entry_file_modification() {
        let temp_dir = TempDir::new().unwrap();
        let test_file = temp_dir.path().join("test.desktop");

        // Create initial file
        fs::write(&test_file, "initial content").unwrap();
        let metadata = fs::metadata(&test_file).unwrap();
        let last_modified = metadata.modified().unwrap();

        let desktop_file = create_test_desktop_file();
        let entry = CacheEntry::new(desktop_file, last_modified);

        // Entry should not be expired
        assert!(!entry.is_expired(&test_file, Duration::from_secs(60)));

        // Modify the file
        thread::sleep(Duration::from_millis(10)); // Ensure different timestamp
        fs::write(&test_file, "modified content").unwrap();

        // Entry should now be expired due to file modification
        assert!(entry.is_expired(&test_file, Duration::from_secs(60)));
    }

    #[test]
    fn test_cache_entry_missing_file() {
        let desktop_file = create_test_desktop_file();
        let entry = CacheEntry::new(desktop_file, SystemTime::now());

        let nonexistent_file = PathBuf::from("/nonexistent/file.desktop");

        // Entry should be considered expired if file doesn't exist
        assert!(entry.is_expired(&nonexistent_file, Duration::from_secs(60)));
    }

    #[test]
    fn test_filesystem_cache_save_create_directory() {
        let temp_dir = TempDir::new().unwrap();
        let cache_path = temp_dir.path().join("subdir").join("cache.json");
        let cache = FileSystemCache::new(cache_path.clone());

        // Directory doesn't exist yet
        assert!(!cache_path.parent().unwrap().exists());

        // Save should create the directory
        assert!(cache.save().is_ok());
        assert!(cache_path.parent().unwrap().exists());
        assert!(cache_path.exists());
    }

    #[test]
    fn test_default_memory_cache() {
        let cache = MemoryCache::default();
        assert!(cache.is_empty());
        assert_eq!(cache.len(), 0);
    }
}
