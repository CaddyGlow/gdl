use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use anyhow::{anyhow, Context, Result};
use futures::stream::{self, StreamExt, TryStreamExt};
use indicatif::MultiProgress;
use log::{info, warn};
use reqwest::Client;
use tokio::sync::Mutex;

use crate::cli::DownloadStrategy;
use crate::download::{collect_download_tasks, download_file};
use crate::git::{download_via_git, ensure_git_available, git_available};
use crate::github::{build_file_inventory, fetch_github_contents, parse_github_url};
use crate::paths::{describe_download_target, determine_paths, ensure_directory};
use crate::progress::{format_bytes, DownloadProgress};
use crate::rate_limit::RateLimitTracker;
use crate::types::{DownloadTask, RequestInfo};

pub async fn download_github_path(
    client: &Client,
    url: &str,
    output: Option<&PathBuf>,
    token: Option<&str>,
    parallel: usize,
    rate_limit: Arc<RateLimitTracker>,
    strategy: DownloadStrategy,
    no_cache: bool,
) -> Result<()> {
    let request = parse_github_url(url)?;
    log::debug!("Parsed request info: {:?}", request);

    let start_time = Instant::now();
    let result = match strategy {
        DownloadStrategy::Api => {
            download_via_rest(
                client,
                &request,
                url,
                output,
                token,
                parallel,
                Arc::clone(&rate_limit),
                no_cache,
            )
            .await
        }
        DownloadStrategy::Git => {
            ensure_git_available()?;
            download_via_git(&request, url, output, token).await
        }
        DownloadStrategy::Auto => {
            match download_via_rest(
                client,
                &request,
                url,
                output,
                token,
                parallel,
                Arc::clone(&rate_limit),
                no_cache,
            )
            .await
            {
                Ok(()) => Ok(()),
                Err(api_err) => {
                    if git_available() {
                        warn!(
                            "REST download failed ({}); attempting git sparse checkout...",
                            api_err
                        );
                        match download_via_git(&request, url, output, token).await {
                            Ok(()) => Ok(()),
                            Err(git_err) => {
                                Err(api_err
                                    .context(format!("git fallback also failed: {}", git_err)))
                            }
                        }
                    } else {
                        Err(api_err)
                    }
                }
            }
        }
    };

    let elapsed = start_time.elapsed();
    info!(
        "Strategy {:?} completed in {:.2}s",
        strategy,
        elapsed.as_secs_f64()
    );

    result
}

async fn download_via_rest(
    client: &Client,
    request: &RequestInfo,
    url: &str,
    output: Option<&PathBuf>,
    token: Option<&str>,
    parallel: usize,
    rate_limit: Arc<RateLimitTracker>,
    no_cache: bool,
) -> Result<()> {
    let contents = fetch_github_contents(
        client,
        &request,
        &request.path,
        token,
        Arc::clone(&rate_limit),
        no_cache,
    )
    .await
    .with_context(|| format!("unable to fetch GitHub contents for {}", url))?;

    if contents.is_empty() {
        return Err(anyhow!("No contents returned for the requested path"));
    }

    let (base_path, default_output_dir) = determine_paths(&request, &contents);
    let output_dir = output.cloned().unwrap_or(default_output_dir);

    let target_display = describe_download_target(&output_dir, &base_path, &contents)?;
    let file_inventory = build_file_inventory(
        client,
        &request,
        token,
        &contents,
        Arc::clone(&rate_limit),
        no_cache,
    )
    .await
    .with_context(|| {
        format!(
            "failed to enumerate files for {}/{}:{}:{}",
            request.owner,
            request.repo,
            request.branch,
            if request.path.is_empty() {
                "/".to_string()
            } else {
                request.path.clone()
            }
        )
    })?;
    ensure_directory(&output_dir)?;

    info!(
        "Downloading from {}/{}:{}:{} into {}",
        request.owner,
        request.repo,
        request.branch,
        if request.path.is_empty() {
            "/"
        } else {
            &request.path
        },
        target_display
    );

    let total_files = file_inventory.len();
    let total_bytes = file_inventory.values().filter_map(|meta| meta.size).sum();
    let download_tasks = collect_download_tasks(
        client,
        &request,
        token,
        &output_dir,
        &base_path,
        contents,
        &file_inventory,
        parallel,
        Arc::clone(&rate_limit),
        no_cache,
    )
    .await?;

    let multi = MultiProgress::new();
    let progress = Arc::new(Mutex::new(DownloadProgress::with_multi_progress(
        total_files,
        total_bytes,
        Some(&multi),
    )));

    log::debug!(
        "Prepared {} file(s) totaling {} for download",
        download_tasks.len(),
        format_bytes(total_bytes)
    );

    download_all_files(
        client,
        token,
        download_tasks,
        Arc::clone(&progress),
        parallel,
        Arc::clone(&rate_limit),
        no_cache,
    )
    .await?;

    let (downloaded_files, downloaded_bytes) = {
        let guard = progress.lock().await;
        guard.finish();
        (guard.downloaded_files, guard.downloaded_bytes)
    };

    info!(
        "Finished downloading {} file(s) ({} total) from {}.",
        downloaded_files,
        format_bytes(downloaded_bytes),
        url
    );

    Ok(())
}

async fn download_all_files(
    client: &Client,
    token: Option<&str>,
    tasks: Vec<DownloadTask>,
    progress: Arc<Mutex<DownloadProgress>>,
    parallel: usize,
    rate_limit: Arc<RateLimitTracker>,
    no_cache: bool,
) -> Result<()> {
    let effective_parallel = parallel.max(1);

    stream::iter(tasks.into_iter().map(|task| {
        let http_client = client.clone();
        let progress = Arc::clone(&progress);
        let rate_limit = Arc::clone(&rate_limit);
        async move {
            download_single_file(http_client, token, task, progress, rate_limit, no_cache).await
        }
    }))
    .buffer_unordered(effective_parallel)
    .try_collect::<Vec<_>>()
    .await
    .map(|_| ())
}

async fn download_single_file(
    client: Client,
    token: Option<&str>,
    task: DownloadTask,
    progress: Arc<Mutex<DownloadProgress>>,
    rate_limit: Arc<RateLimitTracker>,
    no_cache: bool,
) -> Result<()> {
    let DownloadTask {
        item,
        target_path,
        size,
    } = task;

    if let Some(parent) = target_path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .with_context(|| format!("failed to create directory {}", parent.display()))?;
    }

    {
        let guard = progress.lock().await;
        guard.log_start(&item.path, &target_path, size);
    }

    download_file(&client, &item, token, &target_path, &rate_limit, no_cache).await?;
    {
        let mut guard = progress.lock().await;
        guard.record_download(&item.path, &target_path, size);
    }
    Ok(())
}
