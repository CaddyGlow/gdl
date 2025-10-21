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
    fs::create_dir_all(&dir).with_context(|| {
        format!(
            "failed to create repos cache directory {}",
            dir.display()
        )
    })?;
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
        fs::remove_dir_all(&repos_dir).with_context(|| {
            format!(
                "failed to remove repos cache {}",
                repos_dir.display()
            )
        })?;
        info!("Cleared repos cache");
    }

    info!("All caches cleared successfully");
    Ok(())
}
