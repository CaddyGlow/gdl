use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use anyhow::{Context, Result};
use futures::stream::{self, StreamExt, TryStreamExt};
use log::{debug, warn};

use crate::github::fetch_github_contents;
use crate::github::types::{ContentType, GitHubContent};
use crate::paths::relative_path;
use crate::types::{DownloadContext, DownloadOptions, DownloadTask, FileMetadata, RequestInfo};

pub async fn collect_download_tasks(
    ctx: &DownloadContext,
    request: &RequestInfo,
    output_dir: &Path,
    base_path: &Path,
    contents: Vec<GitHubContent>,
    files: &HashMap<String, FileMetadata>,
    options: &DownloadOptions<'_>,
) -> Result<Vec<DownloadTask>> {
    collect_download_tasks_inner(
        ctx, request, output_dir, base_path, contents, files, options,
    )
    .await
}

async fn collect_download_tasks_inner(
    ctx: &DownloadContext,
    request: &RequestInfo,
    output_dir: &Path,
    base_path: &Path,
    contents: Vec<GitHubContent>,
    files: &HashMap<String, FileMetadata>,
    options: &DownloadOptions<'_>,
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

    let listing_parallel = ctx.parallel.max(1);

    let sub_results = stream::iter(directories.into_iter().map(|dir_entry| {
        let http_client = ctx.client.clone();
        let dir_path = dir_entry.path.clone();
        let rate_limit = Arc::clone(&ctx.rate_limit);
        async move {
            debug!("Enumerating directory {}", dir_path);
            let sub_contents = fetch_github_contents(
                &http_client,
                request,
                &dir_path,
                options.token,
                Arc::clone(&rate_limit),
                options.no_cache,
            )
            .await
            .with_context(|| format!("unable to fetch contents of {}", dir_path))?;

            // Create a temporary context for recursive calls
            let sub_ctx = DownloadContext {
                client: http_client,
                rate_limit,
                multi: ctx.multi.clone(),
                parallel: ctx.parallel,
            };

            collect_download_tasks_inner(
                &sub_ctx,
                request,
                output_dir,
                base_path,
                sub_contents,
                files,
                options,
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
