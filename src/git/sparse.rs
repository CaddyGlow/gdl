use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context, Result};
use console::style;
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use log::{debug, info, warn};
use sha2::{Digest, Sha256};
use tokio::task::spawn_blocking;

use crate::cache::repos_cache_dir;
use crate::git::utils::{ensure_git_available, run_git_command, run_git_with_progress};
use crate::github::types::{ContentType, GitHubContent};
use crate::paths::{compute_base_and_default_output, ensure_directory, format_path_for_log};
use crate::progress::{format_bytes, DownloadProgress};
use crate::types::{FileCopyTask, RequestInfo, RequestKind};

pub async fn download_via_git(
    request: &RequestInfo,
    url: &str,
    output: Option<&PathBuf>,
    token: Option<&str>,
    force: bool,
    multi: &MultiProgress,
) -> Result<()> {
    let request = request.clone();
    let url = url.to_string();
    let output = output.cloned();
    let token = token.map(|t| t.to_string());
    let multi = multi.clone();

    spawn_blocking(move || download_via_git_blocking(request, url, output, token, force, multi))
        .await
        .map_err(|err| anyhow!("git download task failed: {}", err))??;
    Ok(())
}

fn download_via_git_blocking(
    request: RequestInfo,
    url: String,
    output: Option<PathBuf>,
    token: Option<String>,
    force: bool,
    multi: MultiProgress,
) -> Result<()> {
    ensure_git_available()?;

    let mut repo_url = url::Url::parse(&format!(
        "https://github.com/{}/{}.git",
        request.owner, request.repo
    ))
    .with_context(|| {
        format!(
            "failed to construct repository URL for {}/{}",
            request.owner, request.repo
        )
    })?;

    if let Some(token) = token.as_deref() {
        repo_url
            .set_username(token.trim())
            .map_err(|_| anyhow!("failed to encode token for git authentication"))?;
        repo_url
            .set_password(Some(""))
            .map_err(|_| anyhow!("failed to set git authentication password"))?;
    }

    let repo_url_string = repo_url.to_string();
    let repo_url_display = format!("https://github.com/{}/{}.git", request.owner, request.repo);

    // Use cache directory instead of temp directory
    let cache_dir = repos_cache_dir()?;

    // Create a unique directory name based on owner/repo/branch
    let mut hasher = Sha256::new();
    hasher.update(format!("{}/{}/{}", request.owner, request.repo, request.branch).as_bytes());
    let repo_hash = format!("{:x}", hasher.finalize());
    let repo_name = format!("{}-{}-{}", request.owner, request.repo, &repo_hash[..8]);

    let repo_dir = cache_dir.join(&repo_name);
    let repo_dir_str = repo_dir
        .to_str()
        .ok_or_else(|| anyhow!("cache directory path contains invalid UTF-8"))?;

    // Show stage indicator for git operations
    eprintln!(
        "{} {} Preparing repository...",
        style("[1/2]").bold().dim(),
        style("⟳").cyan()
    );

    // Check if repo already exists and is valid
    let needs_clone = if repo_dir.exists() {
        debug!("Found cached repository at {}", repo_dir.display());
        // Verify it's a valid git repository
        let is_valid = run_git_command(&["rev-parse", "--git-dir"], Some(&repo_dir), &[]).is_ok();
        if is_valid {
            debug!("Cached repository is valid, updating...");

            // Show progress bar during fetch
            let pb = multi.add(ProgressBar::new(100));
            pb.set_style(
                ProgressStyle::default_bar()
                    .template("{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {msg}")
                    .expect("invalid progress bar template")
                    .progress_chars("#>-"),
            );
            pb.set_message(format!("Fetching updates for {}/{}", request.owner, request.repo));

            // Update the repository
            run_git_with_progress(
                &["fetch", "--progress", "--depth=1", "origin", request.branch.as_str()],
                Some(&repo_dir),
                &[7],
                &pb,
            )
            .with_context(|| format!("failed to update cached repository {}", repo_url_display))?;

            pb.finish_and_clear();
            false
        } else {
            debug!("Cached repository is invalid, removing...");
            fs::remove_dir_all(&repo_dir).with_context(|| {
                format!(
                    "failed to remove invalid cached repository {}",
                    repo_dir.display()
                )
            })?;
            true
        }
    } else {
        true
    };

    if needs_clone {
        debug!("Cloning repository into cache...");

        // Show progress bar during clone
        let pb = multi.add(ProgressBar::new(100));
        pb.set_style(
            ProgressStyle::default_bar()
                .template("{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {msg}")
                .expect("invalid progress bar template")
                .progress_chars("#>-"),
        );
        pb.set_message(format!("Cloning {}/{}", request.owner, request.repo));

        let clone_args = vec![
            "clone",
            "--progress",
            "--filter=blob:none",
            "--depth=1",
            "--branch",
            request.branch.as_str(),
            "--single-branch",
            "--no-checkout",
            repo_url_string.as_str(),
            repo_dir_str,
        ];

        run_git_with_progress(&clone_args, None, &[8], &pb)
            .with_context(|| format!("failed to clone {}", repo_url_display))?;

        pb.finish_and_clear();
    }

    let sparse_checkout_needed = !request.path.is_empty() || request.kind == RequestKind::Blob;
    if sparse_checkout_needed {
        if request.kind == RequestKind::Blob {
            run_git_command(
                &["sparse-checkout", "init", "--no-cone"],
                Some(&repo_dir),
                &[],
            )
            .context("failed to initialize sparse checkout (no-cone)")?;
        } else {
            run_git_command(&["sparse-checkout", "init", "--cone"], Some(&repo_dir), &[])
                .context("failed to initialize sparse checkout (cone)")?;
        }

        let sparse_target = request.path.as_str();

        run_git_command(
            &["sparse-checkout", "set", sparse_target],
            Some(&repo_dir),
            &[],
        )
        .with_context(|| format!("failed to configure sparse checkout for {}", sparse_target))?;
    }

    // Show progress bar during checkout
    let pb = multi.add(ProgressBar::new(100));
    pb.set_style(
        ProgressStyle::default_bar()
            .template("{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {msg}")
            .expect("invalid progress bar template")
            .progress_chars("#>-"),
    );
    pb.set_message(format!("Checking out branch {}", request.branch));

    run_git_with_progress(
        &["checkout", "--progress", request.branch.as_str()],
        Some(&repo_dir),
        &[],
        &pb,
    )
    .with_context(|| format!("failed to checkout branch {}", request.branch))?;

    pb.finish_and_clear();

    let treat_as_single_file = request.kind == RequestKind::Blob;
    let (base_path, default_output_dir) =
        compute_base_and_default_output(&request, treat_as_single_file, None);
    let output_dir = output.unwrap_or(default_output_dir);
    ensure_directory(&output_dir)?;

    let tasks = build_git_copy_tasks(&request, &repo_dir, &output_dir, &base_path)?;
    if tasks.is_empty() {
        return Err(anyhow!(
            "No files matched the requested path {} using git",
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
    let total_bytes: u64 = tasks.iter().filter_map(|task| task.size).sum();

    let mut progress = DownloadProgress::with_multi_progress(
        total_files,
        total_bytes,
        Some(&multi),
    );

    let target_display = if total_files == 1 && treat_as_single_file {
        format_path_for_log(&tasks[0].target_path)
    } else {
        format_path_for_log(&output_dir)
    };

    eprintln!(
        "{} {} Copying files...",
        style("[2/2]").bold().dim(),
        style("»").cyan()
    );

    info!(
        "Downloading from {}/{}:{}:{} into {} (git sparse checkout)",
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

    for task in tasks {
        if let Some(parent) = task.target_path.parent() {
            fs::create_dir_all(parent).with_context(|| {
                format!("failed to create output directory {}", parent.display())
            })?;
        }
        progress.log_start(&task.item_path, &task.target_path, task.size);
        fs::copy(&task.source_path, &task.target_path).with_context(|| {
            format!(
                "failed to copy {} to {}",
                task.source_path.display(),
                task.target_path.display()
            )
        })?;
        progress.record_download(&task.item_path, &task.target_path, task.size);
    }

    progress.finish();

    info!(
        "Finished downloading {} file(s) ({} total) from {} using git.",
        progress.downloaded_files,
        format_bytes(progress.downloaded_bytes),
        url
    );

    Ok(())
}

fn build_git_copy_tasks(
    request: &RequestInfo,
    repo_dir: &Path,
    output_dir: &Path,
    base_path: &Path,
) -> Result<Vec<FileCopyTask>> {
    if request.kind == RequestKind::Blob {
        return build_git_file_task(request, repo_dir, output_dir, base_path)
            .map(|task| vec![task]);
    }

    let source_root = if request.path.is_empty() {
        repo_dir.to_path_buf()
    } else {
        repo_dir.join(&request.path)
    };

    if !source_root.exists() {
        return Err(anyhow!(
            "Path {} not found in cloned repository",
            if request.path.is_empty() {
                "/".to_string()
            } else {
                request.path.clone()
            }
        ));
    }

    let mut stack = vec![source_root];
    let mut tasks = Vec::new();

    while let Some(current) = stack.pop() {
        for entry in fs::read_dir(&current)
            .with_context(|| format!("failed to read directory {}", current.display()))?
        {
            let entry = entry.with_context(|| {
                format!("failed to read directory entry in {}", current.display())
            })?;
            let path = entry.path();
            let metadata = fs::symlink_metadata(&path)
                .with_context(|| format!("failed to inspect entry {}", path.display()))?;

            let relative = path.strip_prefix(repo_dir).with_context(|| {
                format!("failed to derive relative path for {}", path.display())
            })?;

            if relative.starts_with(".git") {
                continue;
            }

            if metadata.is_dir() {
                stack.push(path);
                continue;
            }

            if metadata.file_type().is_symlink() {
                warn!(
                    "Skipping symlink {} encountered during git sparse checkout.",
                    path.display()
                );
                continue;
            }

            if !metadata.is_file() {
                continue;
            }

            let repo_relative = relative.to_string_lossy().replace('\\', "/");
            let name = entry
                .file_name()
                .to_str()
                .map(|s| s.to_string())
                .unwrap_or_else(|| repo_relative.clone());
            let content = GitHubContent {
                name,
                path: repo_relative.clone(),
                url: String::new(),
                size: Some(metadata.len()),
                download_url: None,
                content_type: ContentType::File,
                sha: None,
            };
            let relative_target = crate::paths::relative_path(base_path, &content)?;
            let target_path = output_dir.join(&relative_target);
            tasks.push(FileCopyTask {
                item_path: content.path,
                source_path: path,
                target_path,
                size: Some(metadata.len()),
            });
        }
    }

    Ok(tasks)
}

fn build_git_file_task(
    request: &RequestInfo,
    repo_dir: &Path,
    output_dir: &Path,
    base_path: &Path,
) -> Result<FileCopyTask> {
    if request.path.is_empty() {
        return Err(anyhow!("File download requested but no path provided"));
    }

    let source_path = repo_dir.join(&request.path);
    let metadata = fs::metadata(&source_path).with_context(|| {
        format!(
            "requested file {} is not available in sparse checkout",
            source_path.display()
        )
    })?;

    if !metadata.is_file() {
        return Err(anyhow!("requested path {} is not a file", request.path));
    }

    let repo_relative = Path::new(&request.path)
        .to_string_lossy()
        .replace('\\', "/");
    let name = Path::new(&request.path)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or(&repo_relative)
        .to_string();
    let content = GitHubContent {
        name,
        path: repo_relative.clone(),
        url: String::new(),
        size: Some(metadata.len()),
        download_url: None,
        content_type: ContentType::File,
        sha: None,
    };
    let relative_target = crate::paths::relative_path(base_path, &content)?;
    let target_path = output_dir.join(&relative_target);

    Ok(FileCopyTask {
        item_path: content.path,
        source_path,
        target_path,
        size: Some(metadata.len()),
    })
}
