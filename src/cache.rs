use std::env;
use std::fs::{self, File};
use std::io::{self, Write};
use std::path::PathBuf;
use std::time::SystemTime;

use anyhow::{anyhow, Context, Result};
use log::{debug, info};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::utils::system_time_to_secs;

#[derive(Debug, Serialize, Deserialize)]
pub struct CachedResponse {
    pub url: String,
    pub body: Vec<u8>,
    pub etag: Option<String>,
    pub last_modified: Option<String>,
    pub timestamp: u64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PartialDownload {
    pub url: String,
    pub path: PathBuf,
    pub downloaded_bytes: u64,
    pub total_bytes: Option<u64>,
    pub timestamp: u64,
}

fn cache_base_dir() -> Result<PathBuf> {
    let base = env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .filter(|p| !p.as_os_str().is_empty())
        .or_else(|| {
            env::var_os("HOME")
                .map(PathBuf::from)
                .map(|home| home.join(".cache"))
        })
        .ok_or_else(|| {
            anyhow!("Unable to determine cache directory (set XDG_CACHE_HOME or HOME)")
        })?;

    Ok(base.join("gdl"))
}

pub fn responses_cache_dir() -> Result<PathBuf> {
    let dir = cache_base_dir()?.join("responses");
    fs::create_dir_all(&dir).with_context(|| {
        format!(
            "failed to create responses cache directory {}",
            dir.display()
        )
    })?;
    Ok(dir)
}

#[allow(dead_code)]
pub fn downloads_cache_dir() -> Result<PathBuf> {
    let dir = cache_base_dir()?.join("downloads");
    fs::create_dir_all(&dir).with_context(|| {
        format!(
            "failed to create downloads cache directory {}",
            dir.display()
        )
    })?;
    Ok(dir)
}

pub fn repos_cache_dir() -> Result<PathBuf> {
    let dir = cache_base_dir()?.join("repos");
    fs::create_dir_all(&dir)
        .with_context(|| format!("failed to create repos cache directory {}", dir.display()))?;
    Ok(dir)
}

fn cache_key(url: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(url.as_bytes());
    format!("{:x}", hasher.finalize())
}

pub fn load_cached_response(url: &str, ttl_secs: u64) -> Result<Option<CachedResponse>> {
    let key = cache_key(url);
    let path = responses_cache_dir()?.join(format!("{}.json", key));

    let file = match File::open(&path) {
        Ok(file) => file,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(err) => {
            return Err(anyhow!(
                "failed to open cached response file {}: {}",
                path.display(),
                err
            ))
        }
    };

    let cached: CachedResponse = match serde_json::from_reader(file) {
        Ok(cached) => cached,
        Err(err) => {
            debug!(
                "Unable to parse cached response file {}; ignoring: {}",
                path.display(),
                err
            );
            return Ok(None);
        }
    };

    let now = system_time_to_secs(SystemTime::now());
    if now - cached.timestamp > ttl_secs {
        debug!(
            "Cached response for {} expired (age: {}s, ttl: {}s)",
            url,
            now - cached.timestamp,
            ttl_secs
        );
        return Ok(None);
    }

    debug!(
        "Using cached response for {} (age: {}s)",
        url,
        now - cached.timestamp
    );
    Ok(Some(cached))
}

pub fn save_cached_response(cached: &CachedResponse) -> Result<()> {
    let key = cache_key(&cached.url);
    let path = responses_cache_dir()?.join(format!("{}.json", key));
    let tmp_path = path.with_extension("json.tmp");

    let mut file = File::create(&tmp_path).with_context(|| {
        format!(
            "failed to create temporary cache file {}",
            tmp_path.display()
        )
    })?;

    serde_json::to_writer(&mut file, cached)
        .with_context(|| format!("failed to write cached response to {}", tmp_path.display()))?;

    file.flush()
        .with_context(|| format!("failed to flush cache file {}", tmp_path.display()))?;

    if path.exists() {
        fs::remove_file(&path)
            .with_context(|| format!("failed to remove existing cache file {}", path.display()))?;
    }

    fs::rename(&tmp_path, &path)
        .with_context(|| format!("failed to persist cache file {}", path.display()))?;

    debug!("Saved cached response for {}", cached.url);
    Ok(())
}

pub fn clear_all_caches() -> Result<()> {
    let base = cache_base_dir()?;

    info!("Clearing all cached data from {}", base.display());

    let responses_dir = base.join("responses");
    if responses_dir.exists() {
        fs::remove_dir_all(&responses_dir).with_context(|| {
            format!(
                "failed to remove responses cache {}",
                responses_dir.display()
            )
        })?;
        info!("Cleared response cache");
    }

    let downloads_dir = base.join("downloads");
    if downloads_dir.exists() {
        fs::remove_dir_all(&downloads_dir).with_context(|| {
            format!(
                "failed to remove downloads cache {}",
                downloads_dir.display()
            )
        })?;
        info!("Cleared downloads cache");
    }

    let repos_dir = base.join("repos");
    if repos_dir.exists() {
        fs::remove_dir_all(&repos_dir)
            .with_context(|| format!("failed to remove repos cache {}", repos_dir.display()))?;
        info!("Cleared repos cache");
    }

    info!("All caches cleared successfully");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;
    use std::env;

    fn setup_test_cache_dir() -> PathBuf {
        let test_dir = env::temp_dir().join(format!("gdl_test_{}", std::process::id()));
        unsafe {
            env::set_var("XDG_CACHE_HOME", &test_dir);
        }
        test_dir
    }

    fn cleanup_test_cache_dir(dir: PathBuf) {
        let _ = fs::remove_dir_all(dir);
        unsafe {
            env::remove_var("XDG_CACHE_HOME");
        }
    }

    #[test]
    fn test_cache_key_consistency() {
        let url = "https://api.github.com/repos/owner/repo/contents";
        let key1 = cache_key(url);
        let key2 = cache_key(url);
        assert_eq!(key1, key2);
        assert_eq!(key1.len(), 64); // SHA256 produces 64 hex chars
    }

    #[test]
    fn test_cache_key_uniqueness() {
        let url1 = "https://api.github.com/repos/owner/repo1/contents";
        let url2 = "https://api.github.com/repos/owner/repo2/contents";
        let key1 = cache_key(url1);
        let key2 = cache_key(url2);
        assert_ne!(key1, key2);
    }

    #[test]
    #[serial]
    fn test_save_and_load_cached_response() {
        let test_dir = setup_test_cache_dir();

        // Ensure cache directory exists
        responses_cache_dir().expect("Failed to create cache dir");

        let cached = CachedResponse {
            url: "https://example.com/test".to_string(),
            body: b"test body".to_vec(),
            etag: Some("etag123".to_string()),
            last_modified: Some("Mon, 01 Jan 2024 00:00:00 GMT".to_string()),
            timestamp: system_time_to_secs(SystemTime::now()),
        };

        // Save
        save_cached_response(&cached).expect("Failed to save cache");

        // Load with TTL that won't expire
        let loaded = load_cached_response(&cached.url, 3600)
            .expect("Failed to load cache")
            .expect("Cache should exist");

        assert_eq!(loaded.url, cached.url);
        assert_eq!(loaded.body, cached.body);
        assert_eq!(loaded.etag, cached.etag);
        assert_eq!(loaded.last_modified, cached.last_modified);

        cleanup_test_cache_dir(test_dir);
    }

    #[test]
    #[serial]
    fn test_load_cached_response_expired() {
        let test_dir = setup_test_cache_dir();

        // Ensure cache directory exists
        responses_cache_dir().expect("Failed to create cache dir");

        let cached = CachedResponse {
            url: "https://example.com/test_expired".to_string(),
            body: b"test body".to_vec(),
            etag: Some("etag123".to_string()),
            last_modified: None,
            timestamp: system_time_to_secs(SystemTime::now()) - 7200, // 2 hours ago
        };

        save_cached_response(&cached).expect("Failed to save cache");

        // Load with TTL of 1 hour (cache should be expired)
        let loaded = load_cached_response(&cached.url, 3600).expect("Should not error");
        assert!(loaded.is_none(), "Cache should be expired");

        cleanup_test_cache_dir(test_dir);
    }

    #[test]
    #[serial]
    fn test_load_cached_response_not_found() {
        let test_dir = setup_test_cache_dir();

        let result = load_cached_response("https://nonexistent.com/foo", 3600)
            .expect("Should not error on missing cache");
        assert!(result.is_none());

        cleanup_test_cache_dir(test_dir);
    }

    #[test]
    #[serial]
    fn test_responses_cache_dir_creation() {
        let test_dir = setup_test_cache_dir();

        let cache_dir = responses_cache_dir().expect("Failed to get cache dir");
        assert!(cache_dir.exists(), "Cache directory should be created");
        assert!(cache_dir.ends_with("gdl/responses"));

        cleanup_test_cache_dir(test_dir);
    }

    #[test]
    #[serial]
    fn test_downloads_cache_dir_creation() {
        let test_dir = setup_test_cache_dir();

        let cache_dir = downloads_cache_dir().expect("Failed to get cache dir");
        assert!(cache_dir.exists(), "Downloads cache directory should be created");
        assert!(cache_dir.ends_with("gdl/downloads"));

        cleanup_test_cache_dir(test_dir);
    }

    #[test]
    #[serial]
    fn test_repos_cache_dir_creation() {
        let test_dir = setup_test_cache_dir();

        let cache_dir = repos_cache_dir().expect("Failed to get cache dir");
        assert!(cache_dir.exists(), "Repos cache directory should be created");
        assert!(cache_dir.ends_with("gdl/repos"));

        cleanup_test_cache_dir(test_dir);
    }

    #[test]
    #[serial]
    fn test_clear_all_caches() {
        let test_dir = setup_test_cache_dir();

        // Create cache directories with some files
        let responses_dir = responses_cache_dir().expect("Failed to create responses dir");
        let downloads_dir = downloads_cache_dir().expect("Failed to create downloads dir");
        let repos_dir = repos_cache_dir().expect("Failed to create repos dir");

        // Ensure directories exist before writing files
        fs::create_dir_all(&responses_dir).expect("Failed to create responses dir");
        fs::create_dir_all(&downloads_dir).expect("Failed to create downloads dir");
        fs::create_dir_all(&repos_dir).expect("Failed to create repos dir");

        fs::write(responses_dir.join("test.json"), b"test").expect("Failed to write test file");
        fs::write(downloads_dir.join("test.partial"), b"test").expect("Failed to write test file");
        fs::write(repos_dir.join("test.zip"), b"test").expect("Failed to write test file");

        assert!(responses_dir.exists());
        assert!(downloads_dir.exists());
        assert!(repos_dir.exists());

        // Clear caches
        clear_all_caches().expect("Failed to clear caches");

        // Verify directories are removed
        assert!(!responses_dir.exists());
        assert!(!downloads_dir.exists());
        assert!(!repos_dir.exists());

        cleanup_test_cache_dir(test_dir);
    }

    #[test]
    fn test_cached_response_serialization() {
        let cached = CachedResponse {
            url: "https://example.com/test".to_string(),
            body: vec![1, 2, 3, 4, 5],
            etag: Some("abc123".to_string()),
            last_modified: Some("timestamp".to_string()),
            timestamp: 1234567890,
        };

        let serialized = serde_json::to_string(&cached).expect("Failed to serialize");
        let deserialized: CachedResponse =
            serde_json::from_str(&serialized).expect("Failed to deserialize");

        assert_eq!(deserialized.url, cached.url);
        assert_eq!(deserialized.body, cached.body);
        assert_eq!(deserialized.etag, cached.etag);
        assert_eq!(deserialized.last_modified, cached.last_modified);
        assert_eq!(deserialized.timestamp, cached.timestamp);
    }
}
