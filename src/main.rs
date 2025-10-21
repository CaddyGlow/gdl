use std::collections::HashMap;
use std::env;
use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{anyhow, Context, Result};
use clap::{ArgAction, Parser};
use futures::stream::{self, StreamExt, TryStreamExt};
use log::{debug, info, warn};
use reqwest::header::{HeaderMap, ACCEPT, AUTHORIZATION, RETRY_AFTER};
use reqwest::{Client, StatusCode};
use self_update::backends::github;
use self_update::update::ReleaseUpdate;
use self_update::version;
use serde::{Deserialize, Serialize};
use tokio::io::AsyncWriteExt;
use tokio::sync::Mutex;
use tokio::time::sleep;

const VERSION: &str = env!("GDL_VERSION");
const LONG_VERSION: &str = env!("GDL_LONG_VERSION");
const PKG_VERSION: &str = env!("CARGO_PKG_VERSION");
const GITHUB_OWNER: &str = "CaddyGlow";
const GITHUB_REPO: &str = "gdl";
const BIN_NAME: &str = "gdl";
const UPDATE_CHECK_INTERVAL_SECS: u64 = 60 * 60;
const POSTPONE_DURATION_SECS: u64 = 24 * 60 * 60;

#[derive(Parser, Debug)]
#[command(
    author,
    version = VERSION,
    long_version = LONG_VERSION,
    about = "Download files or directories from a GitHub repository via the REST API."
)]
struct Cli {
    /// GitHub folder URLs to download from (e.g. https://github.com/owner/repo/tree/branch/path)
    #[arg(
        value_name = "URL",
        num_args = 1..,
        required_unless_present_any = ["self_update", "check_update"]
    )]
    urls: Vec<String>,

    /// Update gdl to the latest release and exit
    #[arg(long)]
    self_update: bool,

    /// Check for a newer gdl release and exit without installing it
    #[arg(long)]
    check_update: bool,

    /// Output directory to place the downloaded files (defaults depend on the request)
    #[arg(long)]
    output: Option<PathBuf>,

    /// GitHub personal access token (falls back to GITHUB_TOKEN or GH_TOKEN env vars)
    #[arg(long)]
    token: Option<String>,

    /// Increase logging verbosity (-v for debug, -vv for trace)
    #[arg(short = 'v', long = "verbose", action = ArgAction::Count)]
    verbose: u8,

    /// Maximum number of files to download concurrently
    #[arg(long, value_name = "N", default_value_t = 4)]
    parallel: usize,
}

#[derive(Debug)]
struct RequestInfo {
    owner: String,
    repo: String,
    branch: String,
    path: String,
    has_trailing_slash: bool,
}

#[derive(Debug, Deserialize)]
struct GitHubContent {
    name: String,
    path: String,
    url: String,
    size: Option<u64>,
    #[serde(rename = "download_url")]
    download_url: Option<String>,
    #[serde(rename = "type")]
    content_type: ContentType,
}

#[derive(Debug, Deserialize)]
struct GitTreeResponse {
    tree: Vec<GitTreeEntry>,
    #[serde(default)]
    truncated: bool,
}

#[derive(Debug, Deserialize)]
struct GitTreeEntry {
    path: String,
    #[serde(rename = "type")]
    entry_type: GitTreeEntryType,
    size: Option<u64>,
}

#[derive(Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
enum GitTreeEntryType {
    Blob,
    Tree,
    Commit,
}

#[derive(Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
enum ContentType {
    File,
    Dir,
    Symlink,
    Submodule,
    #[serde(other)]
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RateLimitSnapshot {
    limit: Option<u64>,
    remaining: Option<u64>,
    used: Option<u64>,
    reset_epoch: Option<u64>,
}

impl RateLimitSnapshot {
    fn from_headers(headers: &HeaderMap) -> Option<Self> {
        let limit = header_value_to_u64(headers, "x-ratelimit-limit");
        let remaining = header_value_to_u64(headers, "x-ratelimit-remaining");
        let used = header_value_to_u64(headers, "x-ratelimit-used");
        let reset_epoch = header_value_to_u64(headers, "x-ratelimit-reset");

        if limit.is_none() && remaining.is_none() && used.is_none() && reset_epoch.is_none() {
            return None;
        }

        Some(Self {
            limit,
            remaining,
            used,
            reset_epoch,
        })
    }

    fn reset_eta_display(&self) -> String {
        self.reset_epoch
            .and_then(|epoch| {
                let reset_time = UNIX_EPOCH + Duration::from_secs(epoch);
                reset_time.duration_since(SystemTime::now()).ok()
            })
            .map(|duration| format!("in {}s", duration.as_secs()))
            .unwrap_or_else(|| "at an unknown time".to_string())
    }
}

#[derive(Debug, Default)]
struct RateLimitState {
    last_snapshot: Option<RateLimitSnapshot>,
    lowest_remaining: Option<u64>,
    last_warned_remaining: Option<u64>,
}

#[derive(Debug, Default)]
struct RateLimitTracker {
    state: Mutex<RateLimitState>,
}

impl RateLimitTracker {
    async fn record_headers(&self, headers: &HeaderMap) -> Option<(RateLimitSnapshot, bool, bool)> {
        let snapshot = RateLimitSnapshot::from_headers(headers)?;
        let mut state = self.state.lock().await;

        let log_change = state
            .last_snapshot
            .as_ref()
            .map(|previous| previous != &snapshot)
            .unwrap_or(true);
        state.last_snapshot = Some(snapshot.clone());

        if let Some(remaining) = snapshot.remaining {
            state.lowest_remaining = Some(
                state
                    .lowest_remaining
                    .map_or(remaining, |lowest| lowest.min(remaining)),
            );
        }

        let warn_low = if let (Some(limit), Some(remaining)) = (snapshot.limit, snapshot.remaining)
        {
            let threshold = ((limit as f64) * 0.1).ceil() as u64;
            let threshold = threshold.max(50).min(limit);
            if remaining <= threshold {
                let should_warn = state
                    .last_warned_remaining
                    .map_or(true, |previous| remaining < previous);
                if should_warn {
                    state.last_warned_remaining = Some(remaining);
                    true
                } else {
                    false
                }
            } else {
                false
            }
        } else {
            false
        };

        drop(state);

        Some((snapshot, log_change, warn_low))
    }

    fn backoff_duration(status: StatusCode, headers: &HeaderMap) -> Option<Duration> {
        if status == StatusCode::TOO_MANY_REQUESTS {
            if let Some(duration) = parse_retry_after(headers) {
                return Some(duration);
            }
        }

        if status == StatusCode::FORBIDDEN {
            let remaining = header_value_to_u64(headers, "x-ratelimit-remaining");
            if let Some(remaining) = remaining {
                if remaining > 0 {
                    return None;
                }
            } else {
                return None;
            }
        } else if status != StatusCode::TOO_MANY_REQUESTS {
            return None;
        }

        if let Some(duration) = parse_retry_after(headers) {
            return Some(duration);
        }

        if let Some(reset_epoch) = header_value_to_u64(headers, "x-ratelimit-reset") {
            let reset_time = UNIX_EPOCH + Duration::from_secs(reset_epoch);
            if let Ok(duration) = reset_time.duration_since(SystemTime::now()) {
                if duration > Duration::from_secs(0) {
                    return Some(duration + Duration::from_secs(1));
                }
            }
        }

        None
    }
}

fn header_value_to_u64(headers: &HeaderMap, name: &str) -> Option<u64> {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .and_then(|str_value| str_value.parse::<u64>().ok())
}

fn parse_retry_after(headers: &HeaderMap) -> Option<Duration> {
    let value = headers.get(RETRY_AFTER)?.to_str().ok()?;
    value.parse::<u64>().ok().map(Duration::from_secs)
}

async fn send_github_request(
    builder: &reqwest::RequestBuilder,
    rate_limit: &Arc<RateLimitTracker>,
    context: &str,
) -> Result<reqwest::Response> {
    const MAX_ATTEMPTS: usize = 5;

    for attempt in 1..=MAX_ATTEMPTS {
        let request = builder
            .try_clone()
            .ok_or_else(|| anyhow!("failed to clone GitHub request for {}", context))?;

        let response = request
            .send()
            .await
            .with_context(|| format!("GitHub request failed for {}", context))?;

        if let Some((snapshot, log_change, warn_low)) =
            rate_limit.record_headers(response.headers()).await
        {
            if log_change {
                debug!(
                    "GitHub rate limit: {} remaining of {} (used: {}) - resets {}",
                    snapshot
                        .remaining
                        .map(|value| value.to_string())
                        .unwrap_or_else(|| "unknown".to_string()),
                    snapshot
                        .limit
                        .map(|value| value.to_string())
                        .unwrap_or_else(|| "unknown".to_string()),
                    snapshot
                        .used
                        .map(|value| value.to_string())
                        .unwrap_or_else(|| "unknown".to_string()),
                    snapshot.reset_eta_display()
                );
            }

            if warn_low {
                warn!(
                    "GitHub rate limit low: {} remaining of {} (resets {}).",
                    snapshot
                        .remaining
                        .map(|value| value.to_string())
                        .unwrap_or_else(|| "unknown".to_string()),
                    snapshot
                        .limit
                        .map(|value| value.to_string())
                        .unwrap_or_else(|| "unknown".to_string()),
                    snapshot.reset_eta_display()
                );
            }
        }

        let status = response.status();
        if status.is_success() {
            return Ok(response);
        }

        if let Some(wait) = RateLimitTracker::backoff_duration(status, response.headers()) {
            if attempt == MAX_ATTEMPTS {
                let body = response
                    .text()
                    .await
                    .unwrap_or_else(|_| "<unable to read response body>".into());
                return Err(anyhow!(
                    "GitHub request {} exceeded rate limit after {} attempts (status {}): {}",
                    context,
                    attempt,
                    status,
                    body
                ));
            }

            let wait_secs = wait.as_secs().max(1);
            warn!(
                "GitHub request {} hit a rate limit (status {}), retrying after {}s...",
                context, status, wait_secs
            );
            sleep(wait).await;
            continue;
        }

        let body = response
            .text()
            .await
            .unwrap_or_else(|_| "<unable to read response body>".into());
        return Err(anyhow!(
            "GitHub request {} failed with status {}: {}",
            context,
            status,
            body
        ));
    }

    Err(anyhow!(
        "GitHub request {} failed after exhausting retries",
        context
    ))
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    init_logging(cli.verbose);

    let Cli {
        urls,
        self_update,
        check_update,
        output,
        token,
        verbose: _,
        parallel,
    } = cli;

    let token = token
        .or_else(|| env::var("GITHUB_TOKEN").ok())
        .or_else(|| env::var("GH_TOKEN").ok());

    if self_update {
        run_self_update(token.as_deref())?;
        return Ok(());
    }

    if check_update {
        check_for_update(token.as_deref())?;
        return Ok(());
    }

    auto_check_for_updates(token.as_deref())?;

    let client = Client::builder()
        .user_agent("gdl-rs (https://github.com/CaddyGlow/gdl)")
        .build()
        .context("failed to construct HTTP client")?;
    let rate_limit = Arc::new(RateLimitTracker::default());

    let parallel = parallel.max(1);

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .context("failed to build async runtime")?;

    let rate_limit_for_runtime = Arc::clone(&rate_limit);

    runtime.block_on(async move {
        let output_ref = output.as_ref();
        let token_ref = token.as_deref();
        let rate_limit = rate_limit_for_runtime;
        for url in urls {
            download_github_path(
                &client,
                &url,
                output_ref,
                token_ref,
                parallel,
                Arc::clone(&rate_limit),
            )
            .await?;
        }
        Ok::<(), anyhow::Error>(())
    })?;

    info!("All downloads completed successfully.");
    Ok(())
}

async fn download_github_path(
    client: &Client,
    url: &str,
    output: Option<&PathBuf>,
    token: Option<&str>,
    parallel: usize,
    rate_limit: Arc<RateLimitTracker>,
) -> Result<()> {
    let request = parse_github_url(url)?;
    debug!("Parsed request info: {:?}", request);

    let contents = fetch_github_contents(
        client,
        &request,
        &request.path,
        token,
        Arc::clone(&rate_limit),
    )
    .await
    .with_context(|| format!("unable to fetch GitHub contents for {}", url))?;

    if contents.is_empty() {
        return Err(anyhow!("No contents returned for the requested path"));
    }

    let (base_path, default_output_dir) = determine_paths(&request, &contents);
    let output_dir = output.cloned().unwrap_or(default_output_dir);

    let target_display = describe_download_target(&output_dir, &base_path, &contents)?;
    let file_inventory =
        build_file_inventory(client, &request, token, &contents, Arc::clone(&rate_limit))
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
    )
    .await?;

    let progress = Arc::new(Mutex::new(DownloadProgress::new(total_files, total_bytes)));

    debug!(
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
    )
    .await?;

    let (downloaded_files, downloaded_bytes) = {
        let guard = progress.lock().await;
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

fn format_path_for_log(path: &Path) -> String {
    if path.is_absolute() {
        return path.display().to_string();
    }

    match path.components().next() {
        Some(Component::CurDir) | Some(Component::ParentDir) | None => path.display().to_string(),
        _ => format!("./{}", path.display()),
    }
}

fn describe_download_target(
    output_dir: &Path,
    base_path: &Path,
    contents: &[GitHubContent],
) -> Result<String> {
    if contents.len() == 1 && contents[0].content_type == ContentType::File {
        let relative = relative_path(base_path, &contents[0])?;
        let target = output_dir.join(relative);
        Ok(format_path_for_log(&target))
    } else {
        Ok(format_path_for_log(output_dir))
    }
}

async fn build_file_inventory(
    client: &Client,
    request: &RequestInfo,
    token: Option<&str>,
    contents: &[GitHubContent],
    rate_limit: Arc<RateLimitTracker>,
) -> Result<HashMap<String, FileMetadata>> {
    if contents.len() == 1 && contents[0].content_type == ContentType::File {
        let mut map = HashMap::new();
        map.insert(
            contents[0].path.clone(),
            FileMetadata {
                size: contents[0].size,
            },
        );
        return Ok(map);
    }

    let tree = fetch_git_tree(client, request, token, Arc::clone(&rate_limit)).await?;
    if tree.truncated {
        warn!(
            "GitHub tree listing for {}/{} may be incomplete (truncated).",
            request.owner, request.repo
        );
    }

    let mut files = HashMap::new();
    let base_prefix = if request.path.is_empty() {
        String::new()
    } else {
        format!("{}/", request.path)
    };

    for entry in tree.tree {
        if entry.entry_type != GitTreeEntryType::Blob {
            continue;
        }

        let full_path = if base_prefix.is_empty() {
            entry.path.trim_start_matches('/').to_string()
        } else if entry.path.is_empty() {
            request.path.clone()
        } else {
            format!("{}{}", base_prefix, entry.path.trim_start_matches('/'))
        };

        files.insert(full_path, FileMetadata { size: entry.size });
    }

    Ok(files)
}

async fn fetch_git_tree(
    client: &Client,
    request: &RequestInfo,
    token: Option<&str>,
    rate_limit: Arc<RateLimitTracker>,
) -> Result<GitTreeResponse> {
    let tree_spec = if request.path.is_empty() {
        request.branch.clone()
    } else {
        format!("{}:{}", request.branch, request.path)
    };

    let mut api_url = url::Url::parse("https://api.github.com/repos")
        .context("failed to construct GitHub tree URL")?;
    {
        let mut segments = api_url
            .path_segments_mut()
            .map_err(|_| anyhow!("failed to manipulate GitHub tree URL"))?;
        segments.push(&request.owner);
        segments.push(&request.repo);
        segments.push("git");
        segments.push("trees");
        segments.push(&tree_spec);
    }
    api_url.query_pairs_mut().append_pair("recursive", "1");

    let mut request_builder = client.get(api_url);
    if let Some(token) = token {
        request_builder = request_builder.header(AUTHORIZATION, format!("token {}", token.trim()));
    }

    let context = format!(
        "enumerating git tree for {}/{} ({})",
        request.owner, request.repo, tree_spec
    );
    let response = send_github_request(&request_builder, &rate_limit, &context)
        .await
        .context("GitHub git tree request failed")?;

    let body = response
        .bytes()
        .await
        .context("failed to read GitHub git tree response")?;

    let tree: GitTreeResponse =
        serde_json::from_slice(&body).context("failed to decode GitHub tree response")?;
    Ok(tree)
}

#[derive(Debug, Clone)]
struct FileMetadata {
    size: Option<u64>,
}

#[derive(Debug)]
struct DownloadProgress {
    total_files: usize,
    downloaded_files: usize,
    total_bytes: u64,
    downloaded_bytes: u64,
}

impl DownloadProgress {
    fn new(total_files: usize, total_bytes: u64) -> Self {
        Self {
            total_files,
            downloaded_files: 0,
            total_bytes,
            downloaded_bytes: 0,
        }
    }

    fn log_start(&self, item_path: &str, target_path: &Path, size: Option<u64>) {
        let current = self.downloaded_files + 1;
        let total = self.total_files.max(current);
        let size_info = size
            .map(|bytes| format_bytes(bytes))
            .unwrap_or_else(|| "size unknown".to_string());
        info!(
            "Starting ({}/{}) {} -> {} [{}]",
            current,
            total,
            item_path,
            format_path_for_log(target_path),
            size_info
        );
    }

    fn record_download(&mut self, item_path: &str, target_path: &Path, size: Option<u64>) {
        self.downloaded_files += 1;
        if let Some(bytes) = size {
            self.downloaded_bytes = self.downloaded_bytes.saturating_add(bytes);
        }

        let total = self.total_files.max(self.downloaded_files);
        let size_info = match (size, self.total_bytes) {
            (Some(bytes), total_bytes) if total_bytes > 0 => format!(
                "{} ({} / {})",
                format_bytes(bytes),
                format_bytes(self.downloaded_bytes),
                format_bytes(total_bytes)
            ),
            (Some(bytes), _) => format_bytes(bytes),
            (None, total_bytes) if total_bytes > 0 => format!(
                "{} / {}",
                format_bytes(self.downloaded_bytes),
                format_bytes(total_bytes)
            ),
            _ => "size unknown".to_string(),
        };
        info!(
            "({}/{}) {} -> {} [{}]",
            self.downloaded_files,
            total,
            item_path,
            format_path_for_log(target_path),
            size_info
        );
    }
}

fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    if bytes == 0 {
        return "0 B".to_string();
    }

    let exp = (bytes as f64).log(1024.0).floor() as usize;
    let index = exp.min(UNITS.len() - 1);
    let value = bytes as f64 / 1024_f64.powi(index as i32);
    if index == 0 {
        format!("{} {}", bytes, UNITS[index])
    } else {
        format!("{:.1} {}", value, UNITS[index])
    }
}

fn init_logging(verbosity: u8) {
    let default_level = match verbosity {
        0 => "info",
        1 => "debug",
        _ => "trace",
    };

    let env = env_logger::Env::default().default_filter_or(default_level);
    let _ = env_logger::Builder::from_env(env)
        .format_timestamp_secs()
        .try_init();
}

fn run_self_update(token: Option<&str>) -> Result<()> {
    if skip_self_update() {
        info!("Skipping self-update because GDL_SKIP_SELF_UPDATE is set");
        return Ok(());
    }

    let updater = build_updater(token)?;
    let status = updater
        .update()
        .context("failed to download and install the latest gdl release")?;

    if status.updated() {
        info!("Updated gdl to version {}", status.version());
    } else {
        info!("gdl is already up to date (current: {})", status.version());
    }

    Ok(())
}

fn check_for_update(token: Option<&str>) -> Result<()> {
    if skip_self_update() {
        info!("Skipping update check because GDL_SKIP_SELF_UPDATE is set");
        return Ok(());
    }

    let updater = build_updater(token)?;
    let latest = updater
        .get_latest_release()
        .context("failed to fetch latest gdl release information")?;
    let current_version = updater.current_version();

    if version::bump_is_greater(&current_version, &latest.version)
        .context("failed to compare semantic versions")?
    {
        info!(
            "A newer gdl release is available: {} (current: {})",
            latest.version, current_version
        );
    } else {
        info!("gdl is already at the latest version ({})", current_version);
    }

    Ok(())
}

fn build_updater(token: Option<&str>) -> Result<Box<dyn ReleaseUpdate>> {
    let install_path = current_bin_dir()?;
    let mut builder = github::Update::configure();

    builder
        .repo_owner(GITHUB_OWNER)
        .repo_name(GITHUB_REPO)
        .bin_name(BIN_NAME)
        .bin_install_path(&install_path)
        .target(self_update::get_target())
        .show_download_progress(true)
        .no_confirm(true)
        .current_version(PKG_VERSION);

    if let Some(token) = token {
        if !token.trim().is_empty() {
            builder.auth_token(token.trim());
        }
    }

    builder
        .build()
        .context("failed to configure self-update for gdl")
}

fn current_bin_dir() -> Result<PathBuf> {
    let exe = env::current_exe().context("unable to locate current executable path")?;
    let dir = exe
        .parent()
        .ok_or_else(|| anyhow!("unable to determine install directory for gdl"))?;
    Ok(dir.to_path_buf())
}

fn skip_self_update() -> bool {
    env::var("GDL_SKIP_SELF_UPDATE").is_ok()
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct UpdateState {
    last_check: Option<u64>,
    postpone_until: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum UpdateDecision {
    UpdateNow,
    Postpone,
    Discard,
}

fn auto_check_for_updates(token: Option<&str>) -> Result<()> {
    if skip_self_update() {
        return Ok(());
    }

    let state_path = update_state_path()?;
    let mut state = load_update_state(&state_path)?;
    let now = SystemTime::now();

    if let Some(postpone_until_secs) = state.postpone_until {
        let postpone_until = system_time_from_secs(postpone_until_secs);
        if postpone_until > now {
            debug!(
                "Skipping update check because it was postponed until {:?}",
                postpone_until
            );
            return Ok(());
        }
        state.postpone_until = None;
    }

    if let Some(last_check_secs) = state.last_check {
        let last_check = system_time_from_secs(last_check_secs);
        let elapsed = match now.duration_since(last_check) {
            Ok(duration) => duration,
            Err(_) => Duration::from_secs(UPDATE_CHECK_INTERVAL_SECS),
        };

        if elapsed < Duration::from_secs(UPDATE_CHECK_INTERVAL_SECS) {
            debug!(
                "Skipping update check; last check was {:?} seconds ago",
                elapsed.as_secs()
            );
            return Ok(());
        }
    }

    let updater = build_updater(token)?;
    let latest = updater
        .get_latest_release()
        .context("failed to fetch latest gdl release information")?;
    let current_version = updater.current_version();
    let now_secs = system_time_to_secs(now);

    let is_newer = version::bump_is_greater(&current_version, &latest.version)
        .context("failed to compare semantic versions")?;

    if !is_newer {
        state.last_check = Some(now_secs);
        state.postpone_until = None;
        save_update_state(&state_path, &state)?;
        return Ok(());
    }

    if !atty::is(atty::Stream::Stdin) || !atty::is(atty::Stream::Stdout) {
        info!(
            "A newer gdl release is available: {} (current: {}), but cannot prompt in non-interactive mode",
            latest.version, current_version
        );
        state.last_check = Some(now_secs);
        state.postpone_until = None;
        save_update_state(&state_path, &state)?;
        return Ok(());
    }

    println!(
        "A newer gdl release is available: {} (current: {}).",
        latest.version, current_version
    );

    let decision = prompt_for_update()?;

    match decision {
        UpdateDecision::UpdateNow => {
            state.last_check = Some(now_secs);
            state.postpone_until = None;
            save_update_state(&state_path, &state)?;
            run_self_update(token)?;
        }
        UpdateDecision::Postpone => {
            state.last_check = Some(now_secs);
            state.postpone_until = Some(now_secs + POSTPONE_DURATION_SECS);
            save_update_state(&state_path, &state)?;
            info!("Postponed update check for 24 hours.");
        }
        UpdateDecision::Discard => {
            state.last_check = Some(now_secs);
            state.postpone_until = None;
            save_update_state(&state_path, &state)?;
        }
    }

    Ok(())
}

fn prompt_for_update() -> Result<UpdateDecision> {
    loop {
        print!("Would you like to update now? [yes/postpone/discard]: ");
        io::stdout().flush().context("failed to flush stdout")?;
        let mut input = String::new();
        let bytes = io::stdin()
            .read_line(&mut input)
            .context("failed to read user input")?;

        if bytes == 0 {
            info!("No input received; treating as discard.");
            return Ok(UpdateDecision::Discard);
        }

        match input.trim().to_ascii_lowercase().as_str() {
            "y" | "yes" => return Ok(UpdateDecision::UpdateNow),
            "p" | "postpone" => return Ok(UpdateDecision::Postpone),
            "d" | "discard" | "n" | "no" => return Ok(UpdateDecision::Discard),
            _ => {
                println!("Please enter 'yes', 'postpone', or 'discard'.");
            }
        }
    }
}

fn update_state_path() -> Result<PathBuf> {
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

    let dir = base.join("gdl");
    fs::create_dir_all(&dir)
        .with_context(|| format!("failed to create cache directory {}", dir.display()))?;
    Ok(dir.join("update_state.json"))
}

fn load_update_state(path: &Path) -> Result<UpdateState> {
    let file = match File::open(path) {
        Ok(file) => file,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(UpdateState::default()),
        Err(err) => {
            return Err(anyhow!(
                "failed to open update state file {}: {}",
                path.display(),
                err
            ))
        }
    };

    match serde_json::from_reader(file) {
        Ok(state) => Ok(state),
        Err(err) => {
            warn!(
                "Unable to parse update state file {}; resetting tracking: {}",
                path.display(),
                err
            );
            Ok(UpdateState::default())
        }
    }
}

fn save_update_state(path: &Path, state: &UpdateState) -> Result<()> {
    let tmp_path = path.with_extension("json.tmp");
    let mut file = File::create(&tmp_path).with_context(|| {
        format!(
            "failed to create temporary update state file {}",
            tmp_path.display()
        )
    })?;
    serde_json::to_writer_pretty(&mut file, state)
        .with_context(|| format!("failed to write update state to {}", tmp_path.display()))?;
    file.flush()
        .with_context(|| format!("failed to flush update state file {}", tmp_path.display()))?;
    if path.exists() {
        fs::remove_file(path).with_context(|| {
            format!(
                "failed to remove existing update state file {}",
                path.display()
            )
        })?;
    }
    fs::rename(&tmp_path, path)
        .with_context(|| format!("failed to persist update state file {}", path.display()))?;
    Ok(())
}

fn system_time_to_secs(time: SystemTime) -> u64 {
    time.duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

fn system_time_from_secs(secs: u64) -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(secs)
}

fn parse_github_url(raw_url: &str) -> Result<RequestInfo> {
    let parsed = url::Url::parse(raw_url).context("invalid GitHub URL")?;
    let has_trailing_slash = raw_url.ends_with('/');

    let segments: Vec<_> = parsed
        .path_segments()
        .ok_or_else(|| anyhow!("GitHub URL is missing path segments"))?
        .collect();

    if segments.len() < 5 || (segments[2] != "tree" && segments[2] != "blob") {
        return Err(anyhow!(
            "URL must include /tree/ or /blob/ with a branch and path component"
        ));
    }

    let owner = segments[0].to_string();
    let repo = segments[1].to_string();
    let branch = segments[3].to_string();
    let raw_path = segments[4..].join("/");
    let path = raw_path.trim_matches('/').to_string();

    Ok(RequestInfo {
        owner,
        repo,
        branch,
        path,
        has_trailing_slash,
    })
}

async fn fetch_github_contents(
    client: &Client,
    request: &RequestInfo,
    folder_path: &str,
    token: Option<&str>,
    rate_limit: Arc<RateLimitTracker>,
) -> Result<Vec<GitHubContent>> {
    let mut api_url = url::Url::parse(&format!(
        "https://api.github.com/repos/{}/{}/contents",
        request.owner, request.repo
    ))?;

    {
        let mut segments = api_url
            .path_segments_mut()
            .map_err(|_| anyhow!("failed to manipulate GitHub API URL"))?;
        if !folder_path.is_empty() {
            for segment in folder_path.split('/') {
                if !segment.is_empty() {
                    segments.push(segment);
                }
            }
        }
    }

    api_url
        .query_pairs_mut()
        .append_pair("ref", &request.branch);

    let mut request_builder = client.get(api_url);

    if let Some(token) = token {
        request_builder = request_builder.header(AUTHORIZATION, format!("token {}", token.trim()));
    }

    let context = format!(
        "listing contents for {}/{}:{}:{}",
        request.owner,
        request.repo,
        request.branch,
        if folder_path.is_empty() {
            "/"
        } else {
            folder_path
        }
    );

    let response = send_github_request(&request_builder, &rate_limit, &context)
        .await
        .context("GitHub API request failed")?;

    let body = response
        .bytes()
        .await
        .context("failed to read GitHub API response")?;

    let items: Result<Vec<GitHubContent>, _> = serde_json::from_slice(&body);
    match items {
        Ok(list) => Ok(list),
        Err(_) => {
            let single: GitHubContent =
                serde_json::from_slice(&body).context("unable to decode GitHub API response")?;
            Ok(vec![single])
        }
    }
}

fn determine_paths(request: &RequestInfo, contents: &[GitHubContent]) -> (PathBuf, PathBuf) {
    let is_single_file = contents.len() == 1 && contents[0].content_type == ContentType::File;
    if is_single_file {
        let file_path = Path::new(&contents[0].path);
        let base = file_path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_default();
        let default_output = PathBuf::from(".");
        (normalize_base(base), default_output)
    } else {
        let trimmed = request.path.trim_matches('/');
        let base = if trimmed.is_empty() {
            PathBuf::new()
        } else {
            PathBuf::from(trimmed)
        };

        let default_output = if request.has_trailing_slash || base.as_os_str().is_empty() {
            PathBuf::from(".")
        } else {
            base.file_name()
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("."))
        };

        (normalize_base(base), default_output)
    }
}

fn normalize_base(base: PathBuf) -> PathBuf {
    if base.as_os_str().is_empty() {
        PathBuf::new()
    } else {
        base
    }
}

fn ensure_directory(dir: &Path) -> Result<()> {
    if dir.exists() {
        if !dir.is_dir() {
            return Err(anyhow!(
                "output path {} exists but is not a directory",
                dir.display()
            ));
        }
    } else {
        fs::create_dir_all(dir)
            .with_context(|| format!("failed to create output directory {}", dir.display()))?;
    }
    Ok(())
}

#[derive(Debug)]
struct DownloadTask {
    item: GitHubContent,
    target_path: PathBuf,
    size: Option<u64>,
}

async fn collect_download_tasks(
    client: &Client,
    request: &RequestInfo,
    token: Option<&str>,
    output_dir: &Path,
    base_path: &Path,
    contents: Vec<GitHubContent>,
    files: &HashMap<String, FileMetadata>,
    listing_parallel: usize,
    rate_limit: Arc<RateLimitTracker>,
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

async fn download_all_files(
    client: &Client,
    token: Option<&str>,
    tasks: Vec<DownloadTask>,
    progress: Arc<Mutex<DownloadProgress>>,
    parallel: usize,
    rate_limit: Arc<RateLimitTracker>,
) -> Result<()> {
    let effective_parallel = parallel.max(1);

    stream::iter(tasks.into_iter().map(|task| {
        let http_client = client.clone();
        let progress = Arc::clone(&progress);
        let rate_limit = Arc::clone(&rate_limit);
        async move { download_single_file(http_client, token, task, progress, rate_limit).await }
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

    download_file(&client, &item, token, &target_path, &rate_limit).await?;
    {
        let mut guard = progress.lock().await;
        guard.record_download(&item.path, &target_path, size);
    }
    Ok(())
}

fn relative_path(base_path: &Path, item: &GitHubContent) -> Result<PathBuf> {
    let full_path = Path::new(&item.path);
    let mut relative = if base_path.as_os_str().is_empty() {
        full_path.to_path_buf()
    } else {
        full_path
            .strip_prefix(base_path)
            .map(Path::to_path_buf)
            .unwrap_or_else(|_| full_path.to_path_buf())
    };

    if relative.as_os_str().is_empty() {
        relative = PathBuf::from(&item.name);
    }

    let mut sanitized = PathBuf::new();
    for component in relative.components() {
        match component {
            Component::Normal(part) => sanitized.push(part),
            Component::CurDir => continue,
            _ => {
                return Err(anyhow!(
                    "refusing to write outside the output directory ({})",
                    item.path
                ));
            }
        }
    }

    if sanitized.as_os_str().is_empty() {
        sanitized.push(&item.name);
    }

    Ok(sanitized)
}

async fn download_file(
    client: &Client,
    item: &GitHubContent,
    token: Option<&str>,
    target_path: &Path,
    rate_limit: &Arc<RateLimitTracker>,
) -> Result<()> {
    let mut request_builder = if let Some(ref url) = item.download_url {
        client.get(url)
    } else {
        client
            .get(&item.url)
            .header(ACCEPT, "application/vnd.github.v3.raw")
    };

    if let Some(token) = token {
        request_builder = request_builder.header(AUTHORIZATION, format!("token {}", token.trim()));
    }

    let context = format!("downloading {}", item.path);
    let response = send_github_request(&request_builder, rate_limit, &context)
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
    file.flush()
        .await
        .with_context(|| format!("failed to flush downloaded file {}", target_path.display()))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};

    fn make_file(path: &str) -> GitHubContent {
        let name = Path::new(path)
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or(path)
            .to_string();
        GitHubContent {
            name,
            path: path.to_string(),
            url: format!("https://api.example.com/repos/file/{}", path),
            size: Some(42),
            download_url: Some(format!("https://raw.example.com/repos/file/{}", path)),
            content_type: ContentType::File,
        }
    }

    fn make_dir(path: &str) -> GitHubContent {
        let name = Path::new(path)
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or(path)
            .to_string();
        GitHubContent {
            name,
            path: path.to_string(),
            url: format!("https://api.example.com/repos/dir/{}", path),
            size: None,
            download_url: None,
            content_type: ContentType::Dir,
        }
    }

    #[test]
    fn parses_tree_url_with_trailing_slash() {
        let info = parse_github_url("https://github.com/foo/bar/tree/main/path/to/dir/").unwrap();

        assert_eq!(info.owner, "foo");
        assert_eq!(info.repo, "bar");
        assert_eq!(info.branch, "main");
        assert_eq!(info.path, "path/to/dir");
        assert!(info.has_trailing_slash);
    }

    #[test]
    fn rejects_invalid_github_url() {
        let err = parse_github_url("https://github.com/foo/bar").unwrap_err();
        assert!(
            err.to_string()
                .contains("URL must include /tree/ or /blob/"),
            "{}",
            err
        );
    }

    #[test]
    fn determines_output_for_single_file() {
        let request = RequestInfo {
            owner: "foo".into(),
            repo: "bar".into(),
            branch: "main".into(),
            path: "dir/file.txt".into(),
            has_trailing_slash: false,
        };
        let contents = vec![make_file("dir/file.txt")];
        let (_base, output) = determine_paths(&request, &contents);
        assert_eq!(output, PathBuf::from("."));
    }

    #[test]
    fn determine_output_for_directory_without_trailing_slash() {
        let request = RequestInfo {
            owner: "foo".into(),
            repo: "bar".into(),
            branch: "main".into(),
            path: "dir/subdir".into(),
            has_trailing_slash: false,
        };
        let contents = vec![make_dir("dir/subdir")];
        let (_base, output) = determine_paths(&request, &contents);
        assert_eq!(output, PathBuf::from("subdir"));
    }

    #[test]
    fn relative_path_removes_base_prefix() {
        let base = Path::new("dir/subdir");
        let item = make_file("dir/subdir/file.txt");
        let relative = relative_path(base, &item).unwrap();
        assert_eq!(relative, PathBuf::from("file.txt"));
    }

    #[test]
    fn relative_path_rejects_traversal() {
        let base = Path::new("dir");
        let item = make_file("dir/../evil.txt");
        let err = relative_path(base, &item).unwrap_err();
        assert!(err
            .to_string()
            .contains("refusing to write outside the output directory"));
    }
}
