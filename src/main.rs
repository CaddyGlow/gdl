use std::env;
use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Component, Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{anyhow, Context, Result};
use clap::Parser;
use log::{debug, info, warn};
use reqwest::blocking::Client;
use reqwest::header::{ACCEPT, AUTHORIZATION};
use self_update::backends::github;
use self_update::update::ReleaseUpdate;
use self_update::version;
use serde::{Deserialize, Serialize};

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
    /// GitHub folder URL to download files from (e.g. https://github.com/owner/repo/tree/branch/path)
    #[arg(long, required_unless_present_any = ["self_update", "check_update"])]
    url: Option<String>,

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
    #[serde(rename = "download_url")]
    download_url: Option<String>,
    #[serde(rename = "type")]
    content_type: ContentType,
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

fn main() -> Result<()> {
    init_logging();

    let Cli {
        url,
        self_update,
        check_update,
        output,
        token,
    } = Cli::parse();

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

    let url = url.expect("clap enforces --url when no update flag is used");

    auto_check_for_updates(token.as_deref())?;

    let client = Client::builder()
        .user_agent("gdl-rs (https://github.com/CaddyGlow/gdl)")
        .build()
        .context("failed to construct HTTP client")?;

    let request = parse_github_url(&url)?;
    debug!("Parsed request info: {:?}", request);

    let contents = fetch_github_contents(&client, &request, &request.path, token.as_deref())
        .with_context(|| format!("unable to fetch GitHub contents for {}", url))?;

    if contents.is_empty() {
        return Err(anyhow!("No contents returned for the requested path"));
    }

    let (base_path, default_output_dir) = determine_paths(&request, &contents);
    let output_dir = output.unwrap_or(default_output_dir);

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
        output_dir.display()
    );

    process_contents(
        &client,
        &request,
        &output_dir,
        &base_path,
        token.as_deref(),
        contents,
    )?;

    info!("Download completed.");
    Ok(())
}

fn init_logging() {
    let env = env_logger::Env::default().default_filter_or("info");
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
        fs::remove_file(path)
            .with_context(|| format!("failed to remove existing update state file {}", path.display()))?;
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

fn fetch_github_contents(
    client: &Client,
    request: &RequestInfo,
    folder_path: &str,
    token: Option<&str>,
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

    let response = request_builder
        .send()
        .context("GitHub API request failed")?;

    let status = response.status();
    let body = response
        .bytes()
        .context("failed to read GitHub API response")?;

    if !status.is_success() {
        let message = String::from_utf8_lossy(&body);
        return Err(anyhow!(
            "GitHub API responded with status {}: {}",
            status,
            message
        ));
    }

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

fn process_contents(
    client: &Client,
    request: &RequestInfo,
    output_dir: &Path,
    base_path: &Path,
    token: Option<&str>,
    contents: Vec<GitHubContent>,
) -> Result<()> {
    for item in contents {
        match item.content_type {
            ContentType::File => {
                info!("Downloading {}", item.path);
                let relative = relative_path(base_path, &item)?;
                let target_path = output_dir.join(&relative);
                if let Some(parent) = target_path.parent() {
                    fs::create_dir_all(parent).with_context(|| {
                        format!("failed to create directory {}", parent.display())
                    })?;
                }
                download_file(client, &item, token, &target_path)?;
            }
            ContentType::Dir => {
                info!("Entering directory {}", item.path);
                let sub_contents = fetch_github_contents(client, request, &item.path, token)
                    .with_context(|| format!("unable to fetch contents of {}", item.path))?;
                process_contents(client, request, output_dir, base_path, token, sub_contents)?;
            }
            ContentType::Symlink | ContentType::Submodule | ContentType::Other => {
                warn!(
                    "Skipping unsupported content type {:?} at {}",
                    item.content_type, item.path
                );
            }
        }
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

fn download_file(
    client: &Client,
    item: &GitHubContent,
    token: Option<&str>,
    target_path: &Path,
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

    let mut response = request_builder
        .send()
        .with_context(|| format!("failed to download {}", item.path))?;

    let status = response.status();
    if !status.is_success() {
        let body = response
            .text()
            .unwrap_or_else(|_| "<unable to read response body>".into());
        return Err(anyhow!(
            "failed to download {}: status {}: {}",
            item.path,
            status,
            body
        ));
    }

    let mut file = File::create(target_path)
        .with_context(|| format!("failed to create file {}", target_path.display()))?;
    io::copy(&mut response, &mut file)
        .with_context(|| format!("failed to write content to {}", target_path.display()))?;
    file.flush().context("failed to flush downloaded file")?;

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
