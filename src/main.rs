use std::env;
use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Component, Path, PathBuf};

use anyhow::{anyhow, Context, Result};
use clap::Parser;
use log::{debug, info, warn};
use reqwest::blocking::Client;
use reqwest::header::{ACCEPT, AUTHORIZATION};
use serde::Deserialize;

#[derive(Parser, Debug)]
#[command(
    author,
    version,
    about = "Download files or directories from a GitHub repository via the REST API."
)]
struct Cli {
    /// GitHub folder URL to download files from (e.g. https://github.com/owner/repo/tree/branch/path)
    #[arg(long)]
    url: String,

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

    let cli = Cli::parse();

    let token = cli
        .token
        .or_else(|| env::var("GITHUB_TOKEN").ok())
        .or_else(|| env::var("GH_TOKEN").ok());

    let client = Client::builder()
        .user_agent("gdl-rs (https://github.com/rick/gdl)")
        .build()
        .context("failed to construct HTTP client")?;

    let request = parse_github_url(&cli.url)?;
    debug!("Parsed request info: {:?}", request);

    let contents = fetch_github_contents(&client, &request, &request.path, token.as_deref())
        .with_context(|| format!("unable to fetch GitHub contents for {}", cli.url))?;

    if contents.is_empty() {
        return Err(anyhow!("No contents returned for the requested path"));
    }

    let (base_path, default_output_dir) = determine_paths(&request, &contents);
    let output_dir = cli.output.unwrap_or(default_output_dir);

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
    let path = segments[4..].join("/");

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
        request_builder =
            request_builder.header(AUTHORIZATION, format!("token {}", token.trim()));
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
            let single: GitHubContent = serde_json::from_slice(&body)
                .context("unable to decode GitHub API response")?;
            Ok(vec![single])
        }
    }
}

fn determine_paths(
    request: &RequestInfo,
    contents: &[GitHubContent],
) -> (PathBuf, PathBuf) {
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
                .map(|name| PathBuf::from(name))
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
        fs::create_dir_all(dir).with_context(|| {
            format!(
                "failed to create output directory {}",
                dir.display()
            )
        })?;
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
                        format!(
                            "failed to create directory {}",
                            parent.display()
                        )
                    })?;
                }
                download_file(client, &item, token, &target_path)?;
            }
            ContentType::Dir => {
                info!("Entering directory {}", item.path);
                let sub_contents =
                    fetch_github_contents(client, request, &item.path, token)
                        .with_context(|| {
                            format!("unable to fetch contents of {}", item.path)
                        })?;
                process_contents(
                    client,
                    request,
                    output_dir,
                    base_path,
                    token,
                    sub_contents,
                )?;
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
        request_builder =
            request_builder.header(AUTHORIZATION, format!("token {}", token.trim()));
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

    let mut file = File::create(target_path).with_context(|| {
        format!("failed to create file {}", target_path.display())
    })?;
    io::copy(&mut response, &mut file).with_context(|| {
        format!(
            "failed to write content to {}",
            target_path.display()
        )
    })?;
    file.flush().context("failed to flush downloaded file")?;

    Ok(())
}
