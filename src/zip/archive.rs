use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow};
use console::style;
use futures::StreamExt;
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use log::{debug, info, warn};
use reqwest::Client;
use sha2::{Digest, Sha256};

use crate::cache::repos_cache_dir;
use crate::github::types::{ContentType, GitHubContent};
use crate::paths::{compute_base_and_default_output, ensure_directory, format_path_for_log};
use crate::progress::{DownloadProgress, format_bytes};
use crate::rate_limit::RateLimitTracker;
use crate::types::{DownloadOptions, FileCopyTask, RequestInfo, RequestKind};

/// Parameters for zip download implementation (owned values for async execution)
struct ZipDownloadParams {
    client: Client,
    request: RequestInfo,
    url: String,
    output: Option<PathBuf>,
    token: Option<String>,
    rate_limit: std::sync::Arc<RateLimitTracker>,
    no_cache: bool,
    force: bool,
    multi: MultiProgress,
}

pub async fn download_via_zip(
    client: &Client,
    request: &RequestInfo,
    url: &str,
    output: Option<&PathBuf>,
    rate_limit: std::sync::Arc<RateLimitTracker>,
    options: &DownloadOptions<'_>,
    multi: &MultiProgress,
) -> Result<()> {
    let params = ZipDownloadParams {
        client: client.clone(),
        request: request.clone(),
        url: url.to_string(),
        output: output.cloned(),
        token: options.token.map(|t| t.to_string()),
        rate_limit,
        no_cache: options.no_cache,
        force: options.force,
        multi: multi.clone(),
    };

    download_via_zip_impl(params).await
}

async fn download_via_zip_impl(params: ZipDownloadParams) -> Result<()> {
    let ZipDownloadParams {
        client,
        request,
        url,
        output,
        token,
        rate_limit,
        no_cache,
        force,
        multi,
    } = params;

    // Construct the zip download URL
    let zip_url = format!(
        "https://github.com/{}/{}/archive/refs/heads/{}.zip",
        request.owner, request.repo, request.branch
    );

    debug!("Downloading zip archive from {}", zip_url);

    // Use cache directory for zip files
    let cache_dir = repos_cache_dir()?;

    // Create a unique filename based on owner/repo/branch
    let mut hasher = Sha256::new();
    hasher.update(format!("{}/{}/{}", request.owner, request.repo, request.branch).as_bytes());
    let zip_hash = format!("{:x}", hasher.finalize());
    let zip_filename = format!("{}-{}-{}.zip", request.owner, request.repo, &zip_hash[..8]);
    let zip_path = cache_dir.join(&zip_filename);

    // Download the zip file if not cached or if cache is disabled
    if !zip_path.exists() || no_cache {
        eprintln!(
            "{} {} Downloading zip archive...",
            style("[1/2]").bold().dim(),
            style("▼").cyan()
        );
        debug!("Downloading zip archive to {}", zip_path.display());
        download_zip_file(
            &client,
            &zip_url,
            &zip_path,
            token.as_deref(),
            &rate_limit,
            &multi,
        )
        .await?;
    } else {
        eprintln!(
            "{} {} Using cached zip archive",
            style("[1/2]").bold().dim(),
            style("✓").green()
        );
        info!("Using cached zip archive at {}", zip_path.display());
    }

    // Extract the specific files from the zip
    eprintln!(
        "{} {} Extracting files...",
        style("[2/2]").bold().dim(),
        style("»").cyan()
    );
    extract_from_zip(&request, &zip_path, output, &url, force, &multi)?;

    Ok(())
}

async fn download_zip_file(
    client: &Client,
    url: &str,
    dest_path: &Path,
    token: Option<&str>,
    rate_limit: &RateLimitTracker,
    multi: &MultiProgress,
) -> Result<()> {
    let mut req = client.get(url);

    if let Some(token) = token {
        req = req.header("Authorization", format!("token {}", token));
    }

    let response = req
        .send()
        .await
        .with_context(|| format!("failed to send request to {}", url))?;

    // Update rate limit
    rate_limit.record_headers(response.headers()).await;

    if !response.status().is_success() {
        return Err(anyhow!(
            "failed to download zip: HTTP {}",
            response.status()
        ));
    }

    let total_size = response.content_length();

    // Create progress bar or spinner for zip download based on whether we know the size
    let pb = if let Some(size) = total_size {
        info!("Downloading zip archive: {}", format_bytes(size));
        let bar = multi.add(ProgressBar::new(size));
        bar.set_style(
            ProgressStyle::default_bar()
                .template("{msg}\n{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {bytes}/{total_bytes} ({percent}%)")
                .expect("invalid progress bar template")
                .progress_chars("#>-"),
        );
        bar.set_message("Downloading zip archive");
        bar
    } else {
        info!("Downloading zip archive (size unknown)");
        let spinner = multi.add(ProgressBar::new_spinner());
        spinner.set_style(
            ProgressStyle::default_spinner()
                .template("{spinner:.green} [{elapsed_precise}] {msg} [{bytes}]")
                .expect("invalid spinner template"),
        );
        spinner.set_message("Downloading zip archive");
        spinner.enable_steady_tick(std::time::Duration::from_millis(100));
        spinner
    };

    // Create temp file and download
    let temp_path = dest_path.with_extension("zip.tmp");
    let mut file = File::create(&temp_path)
        .with_context(|| format!("failed to create temporary file {}", temp_path.display()))?;

    // Stream the response and update progress
    let mut stream = response.bytes_stream();
    let mut downloaded: u64 = 0;

    while let Some(chunk) = stream.next().await {
        let chunk = chunk.with_context(|| format!("failed to download chunk from {}", url))?;
        file.write_all(&chunk)
            .with_context(|| format!("failed to write to {}", temp_path.display()))?;
        downloaded += chunk.len() as u64;
        pb.set_position(downloaded);
    }

    pb.finish_and_clear();

    // Rename temp file to final path
    fs::rename(&temp_path, dest_path).with_context(|| {
        format!(
            "failed to move {} to {}",
            temp_path.display(),
            dest_path.display()
        )
    })?;

    info!("Downloaded zip archive: {}", format_bytes(downloaded));
    Ok(())
}

fn extract_from_zip(
    request: &RequestInfo,
    zip_path: &Path,
    output: Option<PathBuf>,
    url: &str,
    force: bool,
    multi: &MultiProgress,
) -> Result<()> {
    let file = File::open(zip_path)
        .with_context(|| format!("failed to open zip file {}", zip_path.display()))?;

    let mut archive = zip::ZipArchive::new(file)
        .with_context(|| format!("failed to read zip archive {}", zip_path.display()))?;

    // GitHub zips have a root directory named "{repo}-{branch}/"
    let zip_prefix = format!("{}-{}/", request.repo, request.branch);

    // Build the path we're looking for inside the zip
    let target_path_in_zip = if request.path.is_empty() {
        zip_prefix.clone()
    } else {
        format!("{}{}", zip_prefix, request.path)
    };

    debug!("Looking for path '{}' in zip archive", target_path_in_zip);

    let treat_as_single_file = request.kind == RequestKind::Blob;
    let (base_path, default_output_dir) =
        compute_base_and_default_output(request, treat_as_single_file, None);
    let output_dir = output.unwrap_or(default_output_dir);
    ensure_directory(&output_dir)?;

    // Collect files to extract
    let mut tasks: Vec<FileCopyTask> = Vec::new();
    let mut total_bytes: u64 = 0;

    for i in 0..archive.len() {
        let file = archive
            .by_index(i)
            .with_context(|| format!("failed to access file at index {} in zip", i))?;

        let file_path = file.name().to_string();

        // Check if this file matches our target path
        let matches = if request.kind == RequestKind::Blob {
            // For a single file, exact match
            file_path == target_path_in_zip
        } else {
            // For a directory, file should be under the target path
            file_path.starts_with(&target_path_in_zip) && !file.is_dir()
        };

        if !matches {
            continue;
        }

        // Strip the zip prefix from the path
        let relative_path = file_path.strip_prefix(&zip_prefix).unwrap_or(&file_path);

        debug!("Found matching file in zip: {}", relative_path);

        // Create GitHubContent for compatibility
        let name = Path::new(relative_path)
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or(relative_path)
            .to_string();

        let content = GitHubContent {
            name,
            path: relative_path.to_string(),
            url: String::new(),
            size: Some(file.size()),
            download_url: None,
            content_type: ContentType::File,
            sha: None,
        };

        let relative_target = crate::paths::relative_path(&base_path, &content)?;
        let target_path = output_dir.join(&relative_target);

        total_bytes += file.size();
        tasks.push(FileCopyTask {
            item_path: content.path,
            source_path: PathBuf::new(), // Not used for zip extraction
            target_path,
            size: Some(file.size()),
        });
    }

    if tasks.is_empty() {
        return Err(anyhow!(
            "No files matched the requested path {} in zip archive",
            if request.path.is_empty() {
                "/".to_string()
            } else {
                request.path.clone()
            }
        ));
    }

    // Check for file overwrites before proceeding
    let target_paths = crate::overwrite::collect_target_paths(&tasks);
    crate::overwrite::check_overwrite_permission(&target_paths, force)?;

    let total_files = tasks.len();

    let mut progress = DownloadProgress::with_multi_progress(total_files, total_bytes, Some(multi));

    let target_display = if total_files == 1 && treat_as_single_file {
        format_path_for_log(&tasks[0].target_path)
    } else {
        format_path_for_log(&output_dir)
    };

    info!(
        "Downloading from {}/{}:{}:{} into {} (zip archive)",
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

    // Extract files
    for task in &tasks {
        // Find the file in the archive again
        let zip_path_to_find = format!("{}{}", zip_prefix, task.item_path);

        let mut found = false;
        for i in 0..archive.len() {
            {
                let file = archive
                    .by_index(i)
                    .with_context(|| format!("failed to access file at index {} in zip", i))?;

                if file.name() != zip_path_to_find {
                    continue;
                }
                found = true;
            }
            extract_file_from_zip(&mut archive, i, task, &mut progress)?;
            break;
        }

        if !found {
            warn!("File {} not found in zip during extraction", task.item_path);
        }
    }

    progress.finish();

    info!(
        "Finished downloading {} file(s) ({} total) from {} using zip archive.",
        progress.downloaded_files,
        format_bytes(progress.downloaded_bytes),
        url
    );

    Ok(())
}

fn extract_file_from_zip(
    archive: &mut zip::ZipArchive<File>,
    index: usize,
    task: &FileCopyTask,
    progress: &mut DownloadProgress,
) -> Result<()> {
    let mut file = archive
        .by_index(index)
        .with_context(|| format!("failed to access file at index {} in zip", index))?;

    if let Some(parent) = task.target_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create output directory {}", parent.display()))?;
    }

    progress.log_start(&task.item_path, &task.target_path, task.size);

    let mut output_file = File::create(&task.target_path)
        .with_context(|| format!("failed to create file {}", task.target_path.display()))?;

    io::copy(&mut file, &mut output_file)
        .with_context(|| format!("failed to extract file to {}", task.target_path.display()))?;

    progress.record_download(&task.item_path, &task.target_path, task.size);

    Ok(())
}
