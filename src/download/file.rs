use std::io;
use std::path::Path;
use std::sync::Arc;

use anyhow::{anyhow, Context, Result};
use futures::StreamExt;
use log::{debug, warn};
use reqwest::header::{ACCEPT, AUTHORIZATION, RANGE};
use reqwest::{Client, StatusCode};
use sha1::{Digest, Sha1};
use tokio::io::AsyncWriteExt;

use crate::github::types::GitHubContent;
use crate::http::send_github_request;
use crate::rate_limit::RateLimitTracker;

pub async fn download_file(
    client: &Client,
    item: &GitHubContent,
    token: Option<&str>,
    target_path: &Path,
    rate_limit: &Arc<RateLimitTracker>,
    no_cache: bool,
) -> Result<()> {
    let url = item.download_url.as_ref().unwrap_or(&item.url);

    // Check for partial download to resume
    let (start_byte, partial_file) = if !no_cache {
        check_partial_download(target_path, item.size).await?
    } else {
        (0, None)
    };

    let mut request_builder = if item.download_url.is_some() {
        client.get(url)
    } else {
        client
            .get(url)
            .header(ACCEPT, "application/vnd.github.v3.raw")
    };

    if let Some(token) = token {
        request_builder = request_builder.header(AUTHORIZATION, format!("token {}", token.trim()));
    }

    // Add Range header for resume
    if start_byte > 0 {
        request_builder = request_builder.header(RANGE, format!("bytes={}-", start_byte));
        debug!(
            "Resuming download of {} from byte {}",
            item.path, start_byte
        );
    }

    let context = format!("downloading {}", item.path);
    let response = send_github_request(&request_builder, rate_limit, &context)
        .await
        .with_context(|| format!("failed to download {}", item.path))?;

    let status = response.status();

    // Check if server supports range requests
    let supports_resume = status == StatusCode::PARTIAL_CONTENT;

    if start_byte > 0 && !supports_resume {
        warn!(
            "Server does not support resume for {}, restarting download",
            item.path
        );
        // Delete partial file and start fresh
        if let Some(pf) = partial_file {
            drop(pf);
            let _ = tokio::fs::remove_file(target_path).await;
        }

        // Retry without range header
        let mut fresh_request = if item.download_url.is_some() {
            client.get(url)
        } else {
            client
                .get(url)
                .header(ACCEPT, "application/vnd.github.v3.raw")
        };

        if let Some(token) = token {
            fresh_request = fresh_request.header(AUTHORIZATION, format!("token {}", token.trim()));
        }

        let response = send_github_request(&fresh_request, rate_limit, &context)
            .await
            .with_context(|| format!("failed to download {}", item.path))?;

        let mut file = tokio::fs::File::create(target_path)
            .await
            .with_context(|| format!("failed to create file {}", target_path.display()))?;
        let mut stream = response.bytes_stream();

        while let Some(chunk) = stream.next().await {
            let chunk = chunk.with_context(|| format!("failed to read data for {}", item.path))?;
            file.write_all(&chunk)
                .await
                .with_context(|| format!("failed to write content to {}", target_path.display()))?;
        }
        file.flush().await.with_context(|| {
            format!("failed to flush downloaded file {}", target_path.display())
        })?;
    } else {
        // Use existing file handle or create new one
        let mut file = if let Some(pf) = partial_file {
            pf
        } else {
            tokio::fs::File::create(target_path)
                .await
                .with_context(|| format!("failed to create file {}", target_path.display()))?
        };

        let mut stream = response.bytes_stream();

        while let Some(chunk) = stream.next().await {
            let chunk = chunk.with_context(|| format!("failed to read data for {}", item.path))?;
            file.write_all(&chunk)
                .await
                .with_context(|| format!("failed to write content to {}", target_path.display()))?;
        }
        file.flush().await.with_context(|| {
            format!("failed to flush downloaded file {}", target_path.display())
        })?;
    }

    // Verify file hash if available
    if let Some(ref expected_sha) = item.sha {
        debug!("Verifying hash for {}", item.path);
        if !verify_file_hash(target_path, expected_sha).await? {
            let _ = tokio::fs::remove_file(target_path).await;
            return Err(anyhow!(
                "Hash verification failed for {}: file may be corrupted",
                item.path
            ));
        }
        debug!("Hash verification passed for {}", item.path);
    }

    Ok(())
}

fn calculate_git_blob_sha1(content: &[u8]) -> String {
    let mut hasher = Sha1::new();
    let header = format!("blob {}\0", content.len());
    hasher.update(header.as_bytes());
    hasher.update(content);
    format!("{:x}", hasher.finalize())
}

async fn verify_file_hash(path: &Path, expected_sha: &str) -> Result<bool> {
    let content = tokio::fs::read(path).await.with_context(|| {
        format!(
            "failed to read file {} for hash verification",
            path.display()
        )
    })?;

    let calculated_sha = calculate_git_blob_sha1(&content);
    Ok(calculated_sha == expected_sha)
}

async fn check_partial_download(
    target_path: &Path,
    expected_size: Option<u64>,
) -> Result<(u64, Option<tokio::fs::File>)> {
    // Check if file already exists
    match tokio::fs::metadata(target_path).await {
        Ok(metadata) => {
            if metadata.is_file() {
                let size = metadata.len();

                // If we know the expected size, validate the partial file
                if let Some(expected) = expected_size {
                    if size >= expected {
                        // File is complete or larger than expected, delete and start fresh
                        debug!("Existing file at {} is complete or larger than expected ({} >= {}), replacing",
                               target_path.display(), size, expected);
                        let _ = tokio::fs::remove_file(target_path).await;
                        return Ok((0, None));
                    }
                }

                if size > 0 {
                    // Open file in append mode for resume
                    let file = tokio::fs::OpenOptions::new()
                        .write(true)
                        .append(true)
                        .open(target_path)
                        .await
                        .with_context(|| {
                            format!("failed to open partial file {}", target_path.display())
                        })?;

                    debug!(
                        "Found partial download at {}, {} bytes",
                        target_path.display(),
                        size
                    );
                    return Ok((size, Some(file)));
                }
            }
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            // File doesn't exist, start fresh
            return Ok((0, None));
        }
        Err(e) => {
            return Err(e).with_context(|| {
                format!(
                    "failed to check for partial download at {}",
                    target_path.display()
                )
            });
        }
    }

    Ok((0, None))
}
