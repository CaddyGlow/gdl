use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use anyhow::{Context, Result};
use futures::stream::{self, StreamExt, TryStreamExt};
use log::{debug, warn};
use reqwest::Client;

use crate::github::types::{ContentType, GitHubContent};
use crate::github::fetch_github_contents;
use crate::paths::relative_path;
use crate::rate_limit::RateLimitTracker;
use crate::types::{DownloadTask, FileMetadata, RequestInfo};

pub async fn collect_download_tasks(
    client: &Client,
    request: &RequestInfo,
    token: Option<&str>,
    output_dir: &Path,
    base_path: &Path,
    contents: Vec<GitHubContent>,
    files: &HashMap<String, FileMetadata>,
    listing_parallel: usize,
    rate_limit: Arc<RateLimitTracker>,
    no_cache: bool,
) -> Result<Vec<DownloadTask>> {
    collect_download_tasks_inner(
        client,
        request,
        token,
        output_dir,
        base_path,
        contents,
        files,
        listing_parallel.max(1),
        rate_limit,
        no_cache,
    )
    .await
}

async fn collect_download_tasks_inner(
    client: &Client,
    request: &RequestInfo,
    token: Option<&str>,
    output_dir: &Path,
    base_path: &Path,
    contents: Vec<GitHubContent>,
    files: &HashMap<String, FileMetadata>,
    listing_parallel: usize,
    rate_limit: Arc<RateLimitTracker>,
    no_cache: bool,
) -> Result<Vec<DownloadTask>> {
    let mut tasks = Vec::new();
    let mut directories = Vec::new();

    for item in contents {
        match item.content_type {
            ContentType::File => {
                let relative = relative_path(base_path, &item)?;
                let target_path = output_dir.join(&relative);
                let size = files.get(&item.path).and_then(|meta| meta.size);
                tasks.push(DownloadTask {
                    item,
                    target_path,
                    size,
                });
            }
            ContentType::Dir => {
                directories.push(item);
            }
            ContentType::Symlink | ContentType::Submodule | ContentType::Other => {
                warn!(
                    "Skipping unsupported content type {:?} at {}",
                    item.content_type, item.path
                );
            }
        }
    }

    if directories.is_empty() {
        return Ok(tasks);
    }

    let sub_results = stream::iter(directories.into_iter().map(|dir_entry| {
        let http_client = client.clone();
        let dir_path = dir_entry.path.clone();
        let rate_limit = Arc::clone(&rate_limit);
        async move {
            debug!("Enumerating directory {}", dir_path);
            let sub_contents = fetch_github_contents(
                &http_client,
                request,
                &dir_path,
                token,
                Arc::clone(&rate_limit),
                no_cache,
            )
            .await
            .with_context(|| format!("unable to fetch contents of {}", dir_path))?;
            collect_download_tasks_inner(
                &http_client,
                request,
                token,
                output_dir,
                base_path,
                sub_contents,
                files,
                listing_parallel,
                rate_limit,
                no_cache,
            )
            .await
        }
    }))
    .buffer_unordered(listing_parallel)
    .try_collect::<Vec<_>>()
    .await?;

    for sub in sub_results {
        tasks.extend(sub);
    }

    Ok(tasks)
}
